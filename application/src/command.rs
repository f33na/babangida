//! Командная сторона CQRS: use-cases записи поверх доменных портов. Каждый
//! проходит через доменные инварианты — лимиты и кулдаун проверяет `domain`
//! ([`Invite::issue`]/[`Invite::accept`]), а не хендлеры.

use babangida_domain::auth::{
    AuthError, Credential, CredentialRepository, Password, SESSION_TTL, Session, SessionId,
    SessionRepository, SessionToken,
};
use babangida_domain::community::{
    Group, GroupId, GroupKind, GroupName, GroupPostRepository, GroupRepository, GroupSlug,
    MemberJoined, MemberLeft, MemberRoleChanged, MembershipRole,
};
use babangida_domain::content::{Post, PostBody, PostRepository};
use babangida_domain::identity::{
    Handle, Invite, InviteAccepted, InviteCode, InviteId, InviteIssued, InviteRepository,
    IssuanceContext, User, UserId, UserRepository, UserRole, VerifiedStatus,
};
use babangida_domain::marketplace::{
    Listing, ListingDraft, ListingId, ListingRepository, ListingSold, ListingWithdrawn,
};
use babangida_domain::messaging::{
    Conversation, ConversationId, ConversationRepository, MessageBody, MessageId,
    MessageRepository, MessageSent,
};
use babangida_domain::music::{Track, TrackDraft, TrackId, TrackRepository, TrackWithdrawn};
use babangida_domain::openapi::{
    ApiKey, ApiKeyId, ApiKeyLabel, ApiKeyRepository, ApiKeyRevoked, ApiKeyToken,
};
use babangida_domain::social::{DisplayName, Profile, Subculture};
use babangida_domain::verification::{
    DecisionReason, RequestNote, VerificationApproved, VerificationRejected, VerificationRequest,
    VerificationRequestId, VerificationRequestRepository,
};
use babangida_shared::{Id, Timestamp};

use crate::{
    ApiKeyFactory, ApiKeyHasher, ApplicationError, Clock, GroupMembershipTxFactory,
    InviteCodeFactory, IssueInviteTxFactory, PasswordHasher, RegistrationTxFactory,
    SessionTokenFactory, VerificationDecisionTxFactory,
};

/// Выдать инвайт от имени юзера.
pub struct IssueInviteCommand {
    pub inviter: UserId,
}

/// Use-case выдачи инвайта. Атомарен (ADR-0011): блокировка инвайтера, чтение
/// состояния, доменное решение и вставка — в одной транзакции. Решение принимает
/// домен ([`Invite::issue`]), транзакция — деталь адаптера.
pub struct IssueInvite<T, C, F> {
    tx_factory: T,
    clock: C,
    codes: F,
}

impl<T, C, F> IssueInvite<T, C, F>
where
    T: IssueInviteTxFactory,
    C: Clock,
    F: InviteCodeFactory,
{
    pub fn new(tx_factory: T, clock: C, codes: F) -> Self {
        Self {
            tx_factory,
            clock,
            codes,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: инвайт не выдан (квота/кулдаун), инвайтер не найден
    /// или сбой хранилища.
    pub async fn execute(&self, cmd: IssueInviteCommand) -> Result<InviteIssued, ApplicationError> {
        let mut tx = self.tx_factory.begin().await?;
        let state = tx
            .lock_inviter(cmd.inviter)
            .await?
            .ok_or(ApplicationError::NotFound("inviter"))?;

        let ctx = IssuanceContext {
            quota: state.quota,
            active_count: state.active_count,
            last_issued_at: state.last_issued_at,
            now: self.clock.now(),
        };
        let (invite, event) = Invite::issue(
            InviteId::generate(),
            self.codes.generate(),
            cmd.inviter,
            ctx,
        )?;

        tx.insert_invite(&invite).await?;
        tx.commit().await?;
        Ok(event)
    }
}

/// Принять инвайт по коду.
pub struct AcceptInviteCommand {
    pub code: InviteCode,
    pub acceptor: UserId,
}

/// Use-case приёма инвайта. Гонку двойного приёма ловит адаптер: `save` для
/// принятого инвайта — условный `UPDATE ... WHERE status = 'active'`, иначе `Conflict`.
pub struct AcceptInvite<I, C> {
    invites: I,
    clock: C,
}

impl<I, C> AcceptInvite<I, C>
where
    I: InviteRepository,
    C: Clock,
{
    pub fn new(invites: I, clock: C) -> Self {
        Self { invites, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: инвайт не найден, не активен, либо собственный.
    pub async fn execute(
        &self,
        cmd: AcceptInviteCommand,
    ) -> Result<InviteAccepted, ApplicationError> {
        let mut invite = self
            .invites
            .find_by_code(&cmd.code)
            .await?
            .ok_or(ApplicationError::NotFound("invite"))?;
        let event = invite.accept(cmd.acceptor, self.clock.now())?;
        self.invites.save(&invite).await?;
        Ok(event)
    }
}

/// Зарегистрироваться по инвайту: создать юзера, профиль и учётные данные, пометить
/// инвайт принятым.
pub struct RegisterCommand {
    pub code: InviteCode,
    pub handle: Handle,
    pub display_name: DisplayName,
    pub subculture: Subculture,
    pub password: Password,
}

/// Use-case регистрации. Атомарен: блокировка активного инвайта, создание юзера/профиля/
/// кредов и пометка инвайта — в одной транзакции (порт [`RegistrationTxFactory`]). Пароль
/// хэшируется на границе ([`PasswordHasher`]); юзера без пароля не возникает (ADR-0013).
pub struct Register<T, H, C> {
    tx_factory: T,
    hasher: H,
    clock: C,
}

impl<T, H, C> Register<T, H, C>
where
    T: RegistrationTxFactory,
    H: PasswordHasher,
    C: Clock,
{
    pub fn new(tx_factory: T, hasher: H, clock: C) -> Self {
        Self {
            tx_factory,
            hasher,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: инвайт не найден/не активен, handle занят (`Conflict`),
    /// либо сбой хранилища.
    pub async fn execute(&self, cmd: RegisterCommand) -> Result<User, ApplicationError> {
        let now = self.clock.now();
        let mut tx = self.tx_factory.begin().await?;
        let mut invite = tx
            .take_active_invite(&cmd.code)
            .await?
            .ok_or(ApplicationError::NotFound("invite"))?;

        let user = User::register(Id::generate(), cmd.handle, UserRole::Member, now);
        tx.insert_user(&user).await?;
        let profile = Profile::create(user.id(), cmd.display_name, cmd.subculture);
        tx.insert_profile(&profile).await?;
        let credential = Credential::establish(user.id(), self.hasher.hash(&cmd.password), now);
        tx.insert_credential(&credential).await?;

        invite.accept(user.id(), now)?;
        tx.mark_invite_accepted(&invite).await?;
        tx.commit().await?;
        Ok(user)
    }
}

/// Опубликовать пост.
pub struct CreatePostCommand {
    pub author: UserId,
    pub body: PostBody,
}

/// Use-case публикации поста.
pub struct CreatePost<P, C> {
    posts: P,
    clock: C,
}

impl<P, C> CreatePost<P, C>
where
    P: PostRepository,
    C: Clock,
{
    pub fn new(posts: P, clock: C) -> Self {
        Self { posts, clock }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое хранилища.
    pub async fn execute(&self, cmd: CreatePostCommand) -> Result<Post, ApplicationError> {
        let post = Post::create(Id::generate(), cmd.author, cmd.body, self.clock.now());
        self.posts.save(&post).await?;
        Ok(post)
    }
}

/// Опубликовать пост в сообщество.
pub struct PostToGroupCommand {
    pub author: UserId,
    pub group: GroupId,
    pub body: PostBody,
}

/// Use-case публикации в сообщество. Контент течёт в общую ленту (анти-ВК,
/// ADR-0012); право публикации решает домен ([`Group::authorize_post`]): в закрытой
/// — любой участник, в паблике — модераторы. Пост остаётся обычным `content::Post`
/// (агрегат не меняется), связь с сообществом пишется атомарно в адаптере.
pub struct PostToGroup<G, P, C> {
    groups: G,
    group_posts: P,
    clock: C,
}

impl<G: GroupRepository, P: GroupPostRepository, C: Clock> PostToGroup<G, P, C> {
    pub fn new(groups: G, group_posts: P, clock: C) -> Self {
        Self {
            groups,
            group_posts,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: группы нет, нет права публикации, либо сбой хранилища.
    pub async fn execute(&self, cmd: PostToGroupCommand) -> Result<Post, ApplicationError> {
        let group = self
            .groups
            .find_by_id(cmd.group)
            .await?
            .ok_or(ApplicationError::NotFound("group"))?;
        group.authorize_post(cmd.author)?;
        let post = Post::create(Id::generate(), cmd.author, cmd.body, self.clock.now());
        self.group_posts.publish(&post, cmd.group).await?;
        Ok(post)
    }
}

/// Отправить личное сообщение `recipient`.
pub struct SendMessageCommand {
    pub author: UserId,
    pub recipient: UserId,
    pub body: MessageBody,
}

/// Use-case переписки: диалог открывается при первом сообщении (find-or-open),
/// дальше дописываются сообщения. Инвариант «автор — участник» проверяет домен
/// ([`Conversation::send`]). Уникальность пары участников держит индекс в
/// `infrastructure`; гонку создания дубликата диалога он же и закрывает.
pub struct SendMessage<R, M, C> {
    conversations: R,
    messages: M,
    clock: C,
}

impl<R, M, C> SendMessage<R, M, C>
where
    R: ConversationRepository,
    M: MessageRepository,
    C: Clock,
{
    pub fn new(conversations: R, messages: M, clock: C) -> Self {
        Self {
            conversations,
            messages,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: диалог с самим собой, автор не участник, либо сбой хранилища.
    pub async fn execute(&self, cmd: SendMessageCommand) -> Result<MessageSent, ApplicationError> {
        let now = self.clock.now();
        let conversation = match self
            .conversations
            .find_between(cmd.author, cmd.recipient)
            .await?
        {
            Some(existing) => existing,
            None => {
                let (opened, _) =
                    Conversation::open(ConversationId::generate(), cmd.author, cmd.recipient, now)?;
                self.conversations.save(&opened).await?;
                // Перечитываем: при гонке создания `save` мог не вставить наш диалог
                // (UNIQUE на паре), и сообщение должно лечь в канонический диалог.
                self.conversations
                    .find_between(cmd.author, cmd.recipient)
                    .await?
                    .ok_or(ApplicationError::NotFound("conversation"))?
            }
        };
        let (message, event) =
            conversation.send(cmd.author, cmd.body, MessageId::generate(), now)?;
        self.messages.append(&message).await?;
        Ok(event)
    }
}

/// Основать сообщество (основатель становится владельцем).
pub struct FoundGroupCommand {
    pub founder: UserId,
    pub slug: GroupSlug,
    pub name: GroupName,
    pub kind: GroupKind,
}

/// Use-case основания сообщества. Уникальность слага держит индекс в
/// `infrastructure` (занят → `Conflict`).
pub struct FoundGroup<G, C> {
    groups: G,
    clock: C,
}

impl<G: GroupRepository, C: Clock> FoundGroup<G, C> {
    pub fn new(groups: G, clock: C) -> Self {
        Self { groups, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: слаг занят (`Conflict`) либо сбой хранилища.
    pub async fn execute(&self, cmd: FoundGroupCommand) -> Result<Group, ApplicationError> {
        let (group, _) = Group::found(
            GroupId::generate(),
            cmd.slug,
            cmd.name,
            cmd.kind,
            cmd.founder,
            self.clock.now(),
        );
        self.groups.save(&group).await?;
        Ok(group)
    }
}

/// Самостоятельно вступить в сообщество (паблик).
pub struct JoinGroupCommand {
    pub group: GroupId,
    pub user: UserId,
}

/// Use-case вступления. Атомарен (ADR-0012): группа блокируется на время чтения,
/// доменного решения ([`Group::join`]) и записи — иначе параллельные изменения
/// состава теряются. Решение (можно ли вступить, не дубль ли) принимает домен.
pub struct JoinGroup<T, C> {
    tx_factory: T,
    clock: C,
}

impl<T: GroupMembershipTxFactory, C: Clock> JoinGroup<T, C> {
    pub fn new(tx_factory: T, clock: C) -> Self {
        Self { tx_factory, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: группы нет, вступление закрыто/уже участник, сбой хранилища.
    pub async fn execute(&self, cmd: JoinGroupCommand) -> Result<MemberJoined, ApplicationError> {
        let mut tx = self.tx_factory.begin().await?;
        let mut group = tx
            .lock_group(cmd.group)
            .await?
            .ok_or(ApplicationError::NotFound("group"))?;
        let event = group.join(cmd.user, self.clock.now())?;
        tx.save(&group).await?;
        tx.commit().await?;
        Ok(event)
    }
}

/// Выйти из сообщества.
pub struct LeaveGroupCommand {
    pub group: GroupId,
    pub user: UserId,
}

/// Use-case выхода. Атомарен (ADR-0012): блокировка группы держит инвариант
/// «нельзя оставить группу без владельца» ([`Group::leave`]) под конкуренцией.
pub struct LeaveGroup<T, C> {
    tx_factory: T,
    clock: C,
}

impl<T: GroupMembershipTxFactory, C: Clock> LeaveGroup<T, C> {
    pub fn new(tx_factory: T, clock: C) -> Self {
        Self { tx_factory, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: группы нет, не участник, единственный владелец, сбой хранилища.
    pub async fn execute(&self, cmd: LeaveGroupCommand) -> Result<MemberLeft, ApplicationError> {
        let mut tx = self.tx_factory.begin().await?;
        let mut group = tx
            .lock_group(cmd.group)
            .await?
            .ok_or(ApplicationError::NotFound("group"))?;
        let event = group.leave(cmd.user, self.clock.now())?;
        tx.save(&group).await?;
        tx.commit().await?;
        Ok(event)
    }
}

/// Сменить роль участника (только владелец).
pub struct SetMemberRoleCommand {
    pub group: GroupId,
    pub actor: UserId,
    pub target: UserId,
    pub role: MembershipRole,
}

/// Use-case смены роли. Атомарен (ADR-0012): блокировка группы сериализует смены
/// ролей; права и инвариант последнего владельца — в домене ([`Group::set_role`]).
pub struct SetMemberRole<T, C> {
    tx_factory: T,
    clock: C,
}

impl<T: GroupMembershipTxFactory, C: Clock> SetMemberRole<T, C> {
    pub fn new(tx_factory: T, clock: C) -> Self {
        Self { tx_factory, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: группы нет, нет прав, цель не участник, последний владелец,
    /// либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: SetMemberRoleCommand,
    ) -> Result<MemberRoleChanged, ApplicationError> {
        let mut tx = self.tx_factory.begin().await?;
        let mut group = tx
            .lock_group(cmd.group)
            .await?
            .ok_or(ApplicationError::NotFound("group"))?;
        let event = group.set_role(cmd.actor, cmd.target, cmd.role, self.clock.now())?;
        tx.save(&group).await?;
        tx.commit().await?;
        Ok(event)
    }
}

// --- auth: учётные данные и сессии (ADR-0013) ---

/// Завести/обновить пароль юзера.
pub struct EstablishCredentialCommand {
    pub user: UserId,
    pub password: Password,
}

/// Use-case установки учётных данных: хэширование — на границе ([`PasswordHasher`]),
/// домен хранит только хэш.
pub struct EstablishCredential<R, H, C> {
    credentials: R,
    hasher: H,
    clock: C,
}

impl<R, H, C> EstablishCredential<R, H, C>
where
    R: CredentialRepository,
    H: PasswordHasher,
    C: Clock,
{
    pub fn new(credentials: R, hasher: H, clock: C) -> Self {
        Self {
            credentials,
            hasher,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое хранилища.
    pub async fn execute(&self, cmd: EstablishCredentialCommand) -> Result<(), ApplicationError> {
        let hash = self.hasher.hash(&cmd.password);
        let credential = Credential::establish(cmd.user, hash, self.clock.now());
        self.credentials.save(&credential).await?;
        Ok(())
    }
}

/// Войти по handle и паролю.
pub struct LogInCommand {
    pub handle: Handle,
    pub password: Password,
}

/// Результат логина: токен сессии (клиент кладёт его в куку) и момент истечения.
#[derive(Debug)]
pub struct Authentication {
    pub token: SessionToken,
    pub expires_at: Timestamp,
}

/// Use-case логина. Не различает «нет юзера»/«неверный пароль» — на оба
/// [`AuthError::InvalidCredentials`] (анти-энумерация). Сессия — на [`SESSION_TTL`].
pub struct LogIn<U, R, S, H, F, C> {
    users: U,
    credentials: R,
    sessions: S,
    hasher: H,
    tokens: F,
    clock: C,
}

impl<U, R, S, H, F, C> LogIn<U, R, S, H, F, C>
where
    U: UserRepository,
    R: CredentialRepository,
    S: SessionRepository,
    H: PasswordHasher,
    F: SessionTokenFactory,
    C: Clock,
{
    pub fn new(users: U, credentials: R, sessions: S, hasher: H, tokens: F, clock: C) -> Self {
        Self {
            users,
            credentials,
            sessions,
            hasher,
            tokens,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError::Auth`] при неверных данных; иначе — сбой хранилища.
    pub async fn execute(&self, cmd: LogInCommand) -> Result<Authentication, ApplicationError> {
        let invalid = || ApplicationError::Auth(AuthError::InvalidCredentials);
        let user = self
            .users
            .find_by_handle(&cmd.handle)
            .await?
            .ok_or_else(invalid)?;
        let credential = self
            .credentials
            .find_by_user(user.id())
            .await?
            .ok_or_else(invalid)?;
        if !self.hasher.verify(&cmd.password, credential.hash()) {
            return Err(invalid());
        }
        let now = self.clock.now();
        let token = self.tokens.generate();
        let (session, _issued) = Session::issue(
            SessionId::generate(),
            token.clone(),
            user.id(),
            now,
            SESSION_TTL,
        );
        self.sessions.save(&session).await?;
        Ok(Authentication {
            token,
            expires_at: session.expires_at(),
        })
    }
}

/// Выйти: погасить сессию по токену.
pub struct LogOutCommand {
    pub token: SessionToken,
}

/// Use-case выхода. Идемпотентен: нет такой сессии — не ошибка.
pub struct LogOut<S> {
    sessions: S,
}

impl<S: SessionRepository> LogOut<S> {
    pub fn new(sessions: S) -> Self {
        Self { sessions }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое хранилища.
    pub async fn execute(&self, cmd: LogOutCommand) -> Result<(), ApplicationError> {
        self.sessions.delete(&cmd.token).await?;
        Ok(())
    }
}

/// Распознать текущего юзера по токену сессии (для middleware).
pub struct AuthenticateCommand {
    pub token: SessionToken,
}

/// Текущий аутентифицированный юзер. `verified` несёт гейт привилегий (ADR-0010).
#[derive(Debug)]
pub struct AuthenticatedUser {
    pub user: UserId,
    pub handle: Handle,
    pub verified: VerifiedStatus,
}

/// Use-case распознавания сессии. Проверяет срок ([`Session::is_active`]); истёкшая
/// или отсутствующая сессия (как и исчезнувший юзер) — [`AuthError::Unauthenticated`].
pub struct Authenticate<S, U, C> {
    sessions: S,
    users: U,
    clock: C,
}

impl<S, U, C> Authenticate<S, U, C>
where
    S: SessionRepository,
    U: UserRepository,
    C: Clock,
{
    pub fn new(sessions: S, users: U, clock: C) -> Self {
        Self {
            sessions,
            users,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError::Auth`] (`Unauthenticated`), если сессии нет/истекла или
    /// юзер исчез; иначе — сбой хранилища.
    pub async fn execute(
        &self,
        cmd: AuthenticateCommand,
    ) -> Result<AuthenticatedUser, ApplicationError> {
        let unauth = || ApplicationError::Auth(AuthError::Unauthenticated);
        let session = self
            .sessions
            .find_by_token(&cmd.token)
            .await?
            .ok_or_else(unauth)?;
        if !session.is_active(self.clock.now()) {
            return Err(unauth());
        }
        let user = self
            .users
            .find_by_id(session.user())
            .await?
            .ok_or_else(unauth)?;
        Ok(AuthenticatedUser {
            user: user.id(),
            handle: user.handle().clone(),
            verified: user.verified(),
        })
    }
}

// --- marketplace: барахолка + гейт верификации (ADR-0010/0014) ---

/// Выставить товар на продажу.
pub struct CreateListingCommand {
    pub seller: UserId,
    pub draft: ListingDraft,
}

/// Use-case выставления товара. Гейт верификации (ADR-0010): грузим продавца, читаем
/// его статус и передаём в домен — решает [`Listing::list`]. Анти-ВК: товар живёт в
/// маркете и профиле той же сети.
pub struct CreateListing<U, L, C> {
    users: U,
    listings: L,
    clock: C,
}

impl<U, L, C> CreateListing<U, L, C>
where
    U: UserRepository,
    L: ListingRepository,
    C: Clock,
{
    pub fn new(users: U, listings: L, clock: C) -> Self {
        Self {
            users,
            listings,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: продавца нет, он не верифицирован (`Marketplace`), либо
    /// сбой хранилища.
    pub async fn execute(&self, cmd: CreateListingCommand) -> Result<Listing, ApplicationError> {
        let seller = self
            .users
            .find_by_id(cmd.seller)
            .await?
            .ok_or(ApplicationError::NotFound("seller"))?;
        let (listing, _event) = Listing::list(
            Id::generate(),
            seller.id(),
            seller.verified(),
            cmd.draft,
            self.clock.now(),
        )?;
        self.listings.save(&listing).await?;
        Ok(listing)
    }
}

/// Отметить товар проданным.
pub struct MarkListingSoldCommand {
    pub listing: ListingId,
    pub actor: UserId,
}

/// Use-case «продано». Право — у домена ([`Listing::mark_sold`]): только продавец,
/// только из активного.
pub struct MarkListingSold<L> {
    listings: L,
}

impl<L: ListingRepository> MarkListingSold<L> {
    pub fn new(listings: L) -> Self {
        Self { listings }
    }

    /// # Errors
    /// [`ApplicationError`]: товара нет, не продавец, уже не активен, либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: MarkListingSoldCommand,
    ) -> Result<ListingSold, ApplicationError> {
        let mut listing = self
            .listings
            .find_by_id(cmd.listing)
            .await?
            .ok_or(ApplicationError::NotFound("listing"))?;
        let event = listing.mark_sold(cmd.actor)?;
        self.listings.save(&listing).await?;
        Ok(event)
    }
}

/// Снять товар с продажи.
pub struct WithdrawListingCommand {
    pub listing: ListingId,
    pub actor: UserId,
}

/// Use-case снятия. Право — у домена ([`Listing::withdraw`]): только продавец.
pub struct WithdrawListing<L> {
    listings: L,
}

impl<L: ListingRepository> WithdrawListing<L> {
    pub fn new(listings: L) -> Self {
        Self { listings }
    }

    /// # Errors
    /// [`ApplicationError`]: товара нет, не продавец, уже не активен, либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: WithdrawListingCommand,
    ) -> Result<ListingWithdrawn, ApplicationError> {
        let mut listing = self
            .listings
            .find_by_id(cmd.listing)
            .await?
            .ok_or(ApplicationError::NotFound("listing"))?;
        let event = listing.withdraw(cmd.actor)?;
        self.listings.save(&listing).await?;
        Ok(event)
    }
}

/// Верифицировать юзера — открыть привилегированные зоны (маркет/музыка/API, ADR-0010).
pub struct VerifyUserCommand {
    pub actor: UserId,
    pub target: Handle,
}

/// Use-case выдачи верификации. Ручная модерация маленькой командой (ADR-0010):
/// выдаёт только админ. Сам статус ставит домен ([`User::verify`]).
pub struct VerifyUser<U> {
    users: U,
}

impl<U: UserRepository> VerifyUser<U> {
    pub fn new(users: U) -> Self {
        Self { users }
    }

    /// # Errors
    /// [`ApplicationError`]: актор не админ (`Forbidden`), целевого юзера нет, либо
    /// сбой хранилища.
    pub async fn execute(&self, cmd: VerifyUserCommand) -> Result<User, ApplicationError> {
        let actor = self
            .users
            .find_by_id(cmd.actor)
            .await?
            .ok_or(ApplicationError::NotFound("actor"))?;
        if actor.role() != UserRole::Admin {
            return Err(ApplicationError::Forbidden(
                "верифицировать может только админ",
            ));
        }
        let mut target = self
            .users
            .find_by_handle(&cmd.target)
            .await?
            .ok_or(ApplicationError::NotFound("user"))?;
        target.verify();
        self.users.save(&target).await?;
        Ok(target)
    }
}

/// Подать заявку на верификацию (ADR-0016). Гейт открывают админы рассмотрением;
/// прямой грант [`VerifyUser`] остаётся за админом, а это — путь самого юзера.
pub struct RequestVerificationCommand {
    pub requester: UserId,
    pub note: Option<RequestNote>,
}

/// Use-case подачи заявки. Кросс-агрегатные правила здесь (домен их не видит):
/// уже верифицированному незачем (`Conflict`), вторую открытую заявку нельзя
/// (`Conflict`) — гонку добивает партиал-уникальный индекс в БД (ADR-0016).
pub struct RequestVerification<U, R, C> {
    users: U,
    requests: R,
    clock: C,
}

impl<U, R, C> RequestVerification<U, R, C>
where
    U: UserRepository,
    R: VerificationRequestRepository,
    C: Clock,
{
    pub fn new(users: U, requests: R, clock: C) -> Self {
        Self {
            users,
            requests,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: юзера нет, уже верифицирован/уже есть открытая заявка
    /// (`Conflict`), либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: RequestVerificationCommand,
    ) -> Result<VerificationRequest, ApplicationError> {
        let user = self
            .users
            .find_by_id(cmd.requester)
            .await?
            .ok_or(ApplicationError::NotFound("user"))?;
        if user.verified().is_verified() {
            return Err(ApplicationError::Conflict("юзер уже верифицирован"));
        }
        if self
            .requests
            .find_pending_by_requester(cmd.requester)
            .await?
            .is_some()
        {
            return Err(ApplicationError::Conflict("заявка уже на рассмотрении"));
        }
        let (request, _event) = VerificationRequest::open(
            VerificationRequestId::generate(),
            cmd.requester,
            cmd.note,
            self.clock.now(),
        );
        self.requests.save(&request).await?;
        Ok(request)
    }
}

/// Одобрить заявку на верификацию (только админ).
pub struct ApproveVerificationCommand {
    pub actor: UserId,
    pub request: VerificationRequestId,
    pub reason: Option<DecisionReason>,
}

/// Use-case одобрения. Атомарен (ADR-0016): заявка блокируется, переходит в
/// `Approved`, и заявитель помечается верифицированным ([`User::verify`]) — всё в
/// одной транзакции, иначе возможно полу-состояние (заявка одобрена, юзер нет).
/// Право решать — только у админа; машину состояний держит домен.
pub struct ApproveVerification<T, C> {
    tx_factory: T,
    clock: C,
}

impl<T: VerificationDecisionTxFactory, C: Clock> ApproveVerification<T, C> {
    pub fn new(tx_factory: T, clock: C) -> Self {
        Self { tx_factory, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: актор не админ (`Forbidden`), заявки/юзера нет, заявка
    /// уже рассмотрена (`Verification`), либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: ApproveVerificationCommand,
    ) -> Result<VerificationApproved, ApplicationError> {
        let mut tx = self.tx_factory.begin().await?;
        let actor = tx
            .find_user(cmd.actor)
            .await?
            .ok_or(ApplicationError::NotFound("actor"))?;
        if actor.role() != UserRole::Admin {
            return Err(ApplicationError::Forbidden(
                "рассматривать заявки может только админ",
            ));
        }
        let mut request = tx
            .lock_request(cmd.request)
            .await?
            .ok_or(ApplicationError::NotFound("verification request"))?;
        let event = request.approve(cmd.actor, cmd.reason, self.clock.now())?;
        let mut requester = tx
            .find_user(request.requester())
            .await?
            .ok_or(ApplicationError::NotFound("user"))?;
        requester.verify();
        tx.save_request(&request).await?;
        tx.save_user(&requester).await?;
        tx.commit().await?;
        Ok(event)
    }
}

/// Отклонить заявку на верификацию (только админ).
pub struct RejectVerificationCommand {
    pub actor: UserId,
    pub request: VerificationRequestId,
    pub reason: Option<DecisionReason>,
}

/// Use-case отказа. Через ту же блокирующую транзакцию, что и одобрение (ADR-0016):
/// решения по заявке сериализуются на её строке, поэтому параллельные одобрение и
/// отказ не разъедутся. Меняется только заявка — юзер не трогается.
pub struct RejectVerification<T, C> {
    tx_factory: T,
    clock: C,
}

impl<T: VerificationDecisionTxFactory, C: Clock> RejectVerification<T, C> {
    pub fn new(tx_factory: T, clock: C) -> Self {
        Self { tx_factory, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: актор не админ (`Forbidden`), заявки нет, заявка уже
    /// рассмотрена (`Verification`), либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: RejectVerificationCommand,
    ) -> Result<VerificationRejected, ApplicationError> {
        let mut tx = self.tx_factory.begin().await?;
        let actor = tx
            .find_user(cmd.actor)
            .await?
            .ok_or(ApplicationError::NotFound("actor"))?;
        if actor.role() != UserRole::Admin {
            return Err(ApplicationError::Forbidden(
                "рассматривать заявки может только админ",
            ));
        }
        let mut request = tx
            .lock_request(cmd.request)
            .await?
            .ok_or(ApplicationError::NotFound("verification request"))?;
        let event = request.reject(cmd.actor, cmd.reason, self.clock.now())?;
        tx.save_request(&request).await?;
        tx.commit().await?;
        Ok(event)
    }
}

/// Выпустить трек (ADR-0017). Гейт верификации (ADR-0010): грузим артиста, читаем
/// его статус и передаём в домен — решает [`Track::release`]. Анти-ВК: трек живёт в
/// разделе музыки и профиле той же сети.
pub struct ReleaseTrackCommand {
    pub uploader: UserId,
    pub draft: TrackDraft,
}

/// Use-case релиза трека. По образцу [`CreateListing`]: `application` читает статус
/// верификации (I/O), домен держит гейт инвариантом.
pub struct ReleaseTrack<U, T, C> {
    users: U,
    tracks: T,
    clock: C,
}

impl<U, T, C> ReleaseTrack<U, T, C>
where
    U: UserRepository,
    T: TrackRepository,
    C: Clock,
{
    pub fn new(users: U, tracks: T, clock: C) -> Self {
        Self {
            users,
            tracks,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: артиста нет, он не верифицирован (`Music`), либо сбой
    /// хранилища.
    pub async fn execute(&self, cmd: ReleaseTrackCommand) -> Result<Track, ApplicationError> {
        let uploader = self
            .users
            .find_by_id(cmd.uploader)
            .await?
            .ok_or(ApplicationError::NotFound("uploader"))?;
        let (track, _event) = Track::release(
            Id::generate(),
            uploader.id(),
            uploader.verified(),
            cmd.draft,
            self.clock.now(),
        )?;
        self.tracks.save(&track).await?;
        Ok(track)
    }
}

/// Снять трек с публикации.
pub struct WithdrawTrackCommand {
    pub track: TrackId,
    pub actor: UserId,
}

/// Use-case снятия. Право — у домена ([`Track::withdraw`]): только автор, только из
/// опубликованного.
pub struct WithdrawTrack<T> {
    tracks: T,
}

impl<T: TrackRepository> WithdrawTrack<T> {
    pub fn new(tracks: T) -> Self {
        Self { tracks }
    }

    /// # Errors
    /// [`ApplicationError`]: трека нет, не автор, уже снят, либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: WithdrawTrackCommand,
    ) -> Result<TrackWithdrawn, ApplicationError> {
        let mut track = self
            .tracks
            .find_by_id(cmd.track)
            .await?
            .ok_or(ApplicationError::NotFound("track"))?;
        let event = track.withdraw(cmd.actor)?;
        self.tracks.save(&track).await?;
        Ok(event)
    }
}

/// Выпустить API-ключ (ADR-0018). Гейт верификации (ADR-0010): грузим владельца, читаем
/// статус и передаём в домен — решает [`ApiKey::issue`]. Секрет генерится на границе
/// (фабрика) и хэшируется (хэшер); владельцу возвращается сырой токен один раз.
pub struct IssueApiKeyCommand {
    pub owner: UserId,
    pub label: ApiKeyLabel,
}

/// Раскрытый при выпуске ключ: агрегат + сырой токен (показывается владельцу единожды).
#[derive(Debug)]
pub struct IssuedApiKey {
    pub key: ApiKey,
    pub token: ApiKeyToken,
}

pub struct IssueApiKey<U, K, F, H, C> {
    users: U,
    keys: K,
    factory: F,
    hasher: H,
    clock: C,
}

impl<U, K, F, H, C> IssueApiKey<U, K, F, H, C>
where
    U: UserRepository,
    K: ApiKeyRepository,
    F: ApiKeyFactory,
    H: ApiKeyHasher,
    C: Clock,
{
    pub fn new(users: U, keys: K, factory: F, hasher: H, clock: C) -> Self {
        Self {
            users,
            keys,
            factory,
            hasher,
            clock,
        }
    }

    /// # Errors
    /// [`ApplicationError`]: владельца нет, он не верифицирован (`OpenApi`), либо сбой
    /// хранилища.
    pub async fn execute(&self, cmd: IssueApiKeyCommand) -> Result<IssuedApiKey, ApplicationError> {
        let owner = self
            .users
            .find_by_id(cmd.owner)
            .await?
            .ok_or(ApplicationError::NotFound("owner"))?;
        let token = self.factory.generate();
        let hash = self.hasher.hash(&token);
        let (key, _event) = ApiKey::issue(
            ApiKeyId::generate(),
            owner.id(),
            owner.verified(),
            cmd.label,
            hash,
            self.clock.now(),
        )?;
        self.keys.save(&key).await?;
        Ok(IssuedApiKey { key, token })
    }
}

/// Отозвать API-ключ.
pub struct RevokeApiKeyCommand {
    pub actor: UserId,
    pub key: ApiKeyId,
}

/// Use-case отзыва. Право — у домена ([`ApiKey::revoke`]): только владелец, только из
/// действующего.
pub struct RevokeApiKey<K> {
    keys: K,
}

impl<K: ApiKeyRepository> RevokeApiKey<K> {
    pub fn new(keys: K) -> Self {
        Self { keys }
    }

    /// # Errors
    /// [`ApplicationError`]: ключа нет, не владелец, уже отозван, либо сбой хранилища.
    pub async fn execute(
        &self,
        cmd: RevokeApiKeyCommand,
    ) -> Result<ApiKeyRevoked, ApplicationError> {
        let mut key = self
            .keys
            .find_by_id(cmd.key)
            .await?
            .ok_or(ApplicationError::NotFound("api key"))?;
        let event = key.revoke(cmd.actor)?;
        self.keys.save(&key).await?;
        Ok(event)
    }
}

/// Распознать владельца по предъявленному API-ключу — для экстрактора `/api/v1`.
pub struct AuthenticateApiKeyCommand {
    pub token: ApiKeyToken,
}

/// Use-case аутентификации по ключу. Хэшируем токен и ищем по хэшу; отозванный или
/// неизвестный ключ — [`AuthError::Unauthenticated`] (как сессия, → 401).
pub struct AuthenticateApiKey<K, H> {
    keys: K,
    hasher: H,
}

impl<K: ApiKeyRepository, H: ApiKeyHasher> AuthenticateApiKey<K, H> {
    pub fn new(keys: K, hasher: H) -> Self {
        Self { keys, hasher }
    }

    /// Возвращает [`UserId`] владельца ключа.
    ///
    /// # Errors
    /// [`ApplicationError::Auth`] (`Unauthenticated`), если ключ неизвестен или отозван;
    /// иначе сбой хранилища.
    pub async fn execute(
        &self,
        cmd: AuthenticateApiKeyCommand,
    ) -> Result<UserId, ApplicationError> {
        let unauth = || ApplicationError::Auth(AuthError::Unauthenticated);
        let hash = self.hasher.hash(&cmd.token);
        let key = self.keys.find_by_hash(&hash).await?.ok_or_else(unauth)?;
        if !key.status().is_active() {
            return Err(unauth());
        }
        Ok(key.owner())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use babangida_domain::RepositoryError;
    use babangida_domain::auth::PasswordHash;
    use babangida_domain::community::CommunityError;
    use babangida_domain::content::PostId;
    use babangida_domain::identity::{InviteError, InviteQuota};
    use babangida_domain::marketplace::{ListingTitle, MarketplaceError, Price};
    use babangida_domain::messaging::{Message, MessagingError};
    use babangida_domain::music::{AudioRef, MusicError, TrackStatus, TrackTitle};
    use babangida_domain::openapi::{ApiKeyHash, ApiKeyStatus, OpenApiError};
    use babangida_domain::verification::{RequestStatus, VerificationError};
    use babangida_shared::{Duration, Timestamp};

    use super::*;
    use crate::{
        GroupMembershipTx, GroupMembershipTxFactory, InviterIssuanceState, IssueInviteTx,
        VerificationDecisionTx, VerificationDecisionTxFactory,
    };

    struct FixedClock(Timestamp);
    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            self.0
        }
    }

    struct FixedCode(InviteCode);
    impl InviteCodeFactory for FixedCode {
        fn generate(&self) -> InviteCode {
            self.0.clone()
        }
    }

    // --- фейковая транзакция выдачи ---
    #[derive(Clone)]
    struct FakeTxState {
        state: Option<InviterIssuanceState>,
        inserted: Arc<Mutex<Vec<Invite>>>,
        committed: Arc<AtomicBool>,
    }
    struct FakeTx(FakeTxState);
    #[async_trait]
    impl IssueInviteTx for FakeTx {
        async fn lock_inviter(
            &mut self,
            _inviter: UserId,
        ) -> Result<Option<InviterIssuanceState>, RepositoryError> {
            Ok(self.0.state)
        }
        async fn insert_invite(&mut self, invite: &Invite) -> Result<(), RepositoryError> {
            self.0.inserted.lock().unwrap().push(invite.clone());
            Ok(())
        }
        async fn commit(&mut self) -> Result<(), RepositoryError> {
            self.0.committed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
    struct FakeTxFactory(FakeTxState);
    #[async_trait]
    impl IssueInviteTxFactory for FakeTxFactory {
        async fn begin(&self) -> Result<Box<dyn IssueInviteTx>, RepositoryError> {
            Ok(Box::new(FakeTx(self.0.clone())))
        }
    }

    fn factory(state: Option<InviterIssuanceState>) -> (FakeTxFactory, FakeTxState) {
        let st = FakeTxState {
            state,
            inserted: Arc::new(Mutex::new(Vec::new())),
            committed: Arc::new(AtomicBool::new(false)),
        };
        (FakeTxFactory(st.clone()), st)
    }

    fn state(active: u32, last: Option<Timestamp>) -> InviterIssuanceState {
        InviterIssuanceState {
            quota: InviteQuota::Limited(2),
            active_count: active,
            last_issued_at: last,
        }
    }

    fn code() -> InviteCode {
        InviteCode::parse("ABCD1234").unwrap()
    }

    #[tokio::test]
    async fn issue_invite_succeeds_inserts_and_commits() {
        let now = Timestamp::now();
        let (f, st) = factory(Some(state(0, None)));
        let uc = IssueInvite::new(f, FixedClock(now), FixedCode(code()));
        let event = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap();
        assert_eq!(event.code, code());
        assert_eq!(st.inserted.lock().unwrap().len(), 1);
        assert!(st.committed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn issue_invite_surfaces_quota_error_without_insert() {
        let now = Timestamp::now();
        let (f, st) = factory(Some(state(2, Some(now + Duration::hours(-24)))));
        let uc = IssueInvite::new(f, FixedClock(now), FixedCode(code()));
        let err = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Invite(InviteError::QuotaExceeded { .. })
        ));
        assert_eq!(st.inserted.lock().unwrap().len(), 0);
        assert!(!st.committed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn issue_invite_surfaces_cooldown_error() {
        let now = Timestamp::now();
        let (f, _) = factory(Some(state(0, Some(now + Duration::hours(-1)))));
        let uc = IssueInvite::new(f, FixedClock(now), FixedCode(code()));
        let err = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Invite(InviteError::CooldownActive { .. })
        ));
    }

    #[tokio::test]
    async fn issue_invite_missing_inviter_is_not_found() {
        let now = Timestamp::now();
        let (f, _) = factory(None);
        let uc = IssueInvite::new(f, FixedClock(now), FixedCode(code()));
        let err = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::NotFound("inviter")));
    }

    // --- AcceptInvite поверх InviteRepository ---
    struct FakeInvites {
        by_code: Option<Invite>,
        saved: Mutex<Vec<Invite>>,
    }
    #[async_trait]
    impl InviteRepository for FakeInvites {
        async fn find_by_id(&self, _id: InviteId) -> Result<Option<Invite>, RepositoryError> {
            Ok(None)
        }
        async fn find_by_code(&self, _c: &InviteCode) -> Result<Option<Invite>, RepositoryError> {
            Ok(self.by_code.clone())
        }
        async fn save(&self, invite: &Invite) -> Result<(), RepositoryError> {
            self.saved.lock().unwrap().push(invite.clone());
            Ok(())
        }
        async fn count_active_by_inviter(&self, _i: UserId) -> Result<u32, RepositoryError> {
            Ok(0)
        }
        async fn last_issued_at_by_inviter(
            &self,
            _i: UserId,
        ) -> Result<Option<Timestamp>, RepositoryError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn accept_invite_succeeds() {
        let now = Timestamp::now();
        let inviter = Id::generate();
        let (invite, _) = Invite::issue(
            Id::generate(),
            code(),
            inviter,
            IssuanceContext {
                quota: InviteQuota::Limited(2),
                active_count: 0,
                last_issued_at: None,
                now,
            },
        )
        .unwrap();
        let invites = FakeInvites {
            by_code: Some(invite),
            saved: Mutex::new(Vec::new()),
        };
        let uc = AcceptInvite::new(invites, FixedClock(now));
        let event = uc
            .execute(AcceptInviteCommand {
                code: code(),
                acceptor: Id::generate(),
            })
            .await
            .unwrap();
        assert_eq!(event.inviter, inviter);
        assert_eq!(uc.invites.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn accept_invite_unknown_code_is_not_found() {
        let now = Timestamp::now();
        let invites = FakeInvites {
            by_code: None,
            saved: Mutex::new(Vec::new()),
        };
        let uc = AcceptInvite::new(invites, FixedClock(now));
        let err = uc
            .execute(AcceptInviteCommand {
                code: code(),
                acceptor: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::NotFound("invite")));
    }

    // --- messaging фейки + тесты ---
    struct FakeConversations {
        existing: Mutex<Option<Conversation>>,
    }
    impl FakeConversations {
        fn empty() -> Self {
            Self {
                existing: Mutex::new(None),
            }
        }
    }
    #[async_trait]
    impl ConversationRepository for FakeConversations {
        async fn find_by_id(
            &self,
            _id: ConversationId,
        ) -> Result<Option<Conversation>, RepositoryError> {
            Ok(self.existing.lock().unwrap().clone())
        }
        async fn find_between(
            &self,
            _a: UserId,
            _b: UserId,
        ) -> Result<Option<Conversation>, RepositoryError> {
            Ok(self.existing.lock().unwrap().clone())
        }
        async fn save(&self, conversation: &Conversation) -> Result<(), RepositoryError> {
            *self.existing.lock().unwrap() = Some(conversation.clone());
            Ok(())
        }
    }

    struct FakeMessages {
        appended: Arc<Mutex<Vec<Message>>>,
    }
    #[async_trait]
    impl MessageRepository for FakeMessages {
        async fn append(&self, message: &Message) -> Result<(), RepositoryError> {
            self.appended.lock().unwrap().push(message.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn send_message_opens_conversation_and_appends() {
        let now = Timestamp::now();
        let appended = Arc::new(Mutex::new(Vec::new()));
        let uc = SendMessage::new(
            FakeConversations::empty(),
            FakeMessages {
                appended: appended.clone(),
            },
            FixedClock(now),
        );
        let author = Id::generate();
        let event = uc
            .execute(SendMessageCommand {
                author,
                recipient: Id::generate(),
                body: MessageBody::parse("здарова").unwrap(),
            })
            .await
            .unwrap();
        assert_eq!(event.author, author);
        assert_eq!(appended.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn send_message_to_self_is_rejected() {
        let now = Timestamp::now();
        let appended = Arc::new(Mutex::new(Vec::new()));
        let uc = SendMessage::new(
            FakeConversations::empty(),
            FakeMessages {
                appended: appended.clone(),
            },
            FixedClock(now),
        );
        let me = Id::generate();
        let err = uc
            .execute(SendMessageCommand {
                author: me,
                recipient: me,
                body: MessageBody::parse("сам себе").unwrap(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Messaging(MessagingError::SelfConversation)
        ));
        assert_eq!(appended.lock().unwrap().len(), 0);
    }

    // --- community фейки + тесты ---
    // FoundGroup пишет через GroupRepository (вставка нового сообщества).
    struct FakeGroups {
        saved: Mutex<Option<Group>>,
    }
    impl FakeGroups {
        fn new() -> Self {
            Self {
                saved: Mutex::new(None),
            }
        }
        fn with(group: Group) -> Self {
            Self {
                saved: Mutex::new(Some(group)),
            }
        }
    }

    struct FakeGroupPosts {
        published: Arc<Mutex<Vec<(PostId, GroupId)>>>,
    }
    #[async_trait]
    impl GroupPostRepository for FakeGroupPosts {
        async fn publish(&self, post: &Post, group: GroupId) -> Result<(), RepositoryError> {
            self.published.lock().unwrap().push((post.id(), group));
            Ok(())
        }
    }
    #[async_trait]
    impl GroupRepository for FakeGroups {
        async fn find_by_id(&self, _id: GroupId) -> Result<Option<Group>, RepositoryError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        async fn find_by_slug(&self, _slug: &GroupSlug) -> Result<Option<Group>, RepositoryError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        async fn save(&self, group: &Group) -> Result<(), RepositoryError> {
            *self.saved.lock().unwrap() = Some(group.clone());
            Ok(())
        }
    }

    // Членство (join/leave/set_role) идёт через транзакцию с блокировкой группы.
    #[derive(Clone)]
    struct FakeMembershipState {
        group: Arc<Mutex<Option<Group>>>,
    }
    struct FakeMembershipTx(FakeMembershipState);
    #[async_trait]
    impl GroupMembershipTx for FakeMembershipTx {
        async fn lock_group(&mut self, _id: GroupId) -> Result<Option<Group>, RepositoryError> {
            Ok(self.0.group.lock().unwrap().clone())
        }
        async fn save(&mut self, group: &Group) -> Result<(), RepositoryError> {
            *self.0.group.lock().unwrap() = Some(group.clone());
            Ok(())
        }
        async fn commit(&mut self) -> Result<(), RepositoryError> {
            Ok(())
        }
    }
    struct FakeMembershipFactory(FakeMembershipState);
    #[async_trait]
    impl GroupMembershipTxFactory for FakeMembershipFactory {
        async fn begin(&self) -> Result<Box<dyn GroupMembershipTx>, RepositoryError> {
            Ok(Box::new(FakeMembershipTx(self.0.clone())))
        }
    }
    fn membership(group: Option<Group>) -> FakeMembershipFactory {
        FakeMembershipFactory(FakeMembershipState {
            group: Arc::new(Mutex::new(group)),
        })
    }

    fn public_group(owner: UserId) -> Group {
        Group::found(
            Id::generate(),
            GroupSlug::parse("podval").unwrap(),
            GroupName::parse("Подвал").unwrap(),
            GroupKind::Public,
            owner,
            Timestamp::now(),
        )
        .0
    }

    #[tokio::test]
    async fn found_group_makes_founder_owner() {
        let uc = FoundGroup::new(FakeGroups::new(), FixedClock(Timestamp::now()));
        let founder = Id::generate();
        let group = uc
            .execute(FoundGroupCommand {
                founder,
                slug: GroupSlug::parse("podval").unwrap(),
                name: GroupName::parse("Подвал").unwrap(),
                kind: GroupKind::Public,
            })
            .await
            .unwrap();
        assert_eq!(group.role_of(founder), Some(MembershipRole::Owner));
    }

    #[tokio::test]
    async fn join_public_group_succeeds() {
        let uc = JoinGroup::new(
            membership(Some(public_group(Id::generate()))),
            FixedClock(Timestamp::now()),
        );
        let user = Id::generate();
        let event = uc
            .execute(JoinGroupCommand {
                group: Id::generate(),
                user,
            })
            .await
            .unwrap();
        assert_eq!(event.user, user);
    }

    #[tokio::test]
    async fn join_missing_group_is_not_found() {
        let uc = JoinGroup::new(membership(None), FixedClock(Timestamp::now()));
        let err = uc
            .execute(JoinGroupCommand {
                group: Id::generate(),
                user: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::NotFound("group")));
    }

    #[tokio::test]
    async fn set_role_by_non_owner_is_rejected() {
        let owner = Id::generate();
        let uc = SetMemberRole::new(
            membership(Some(public_group(owner))),
            FixedClock(Timestamp::now()),
        );
        let err = uc
            .execute(SetMemberRoleCommand {
                group: Id::generate(),
                actor: Id::generate(),
                target: owner,
                role: MembershipRole::Member,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Community(CommunityError::NotPermitted)
        ));
    }

    #[tokio::test]
    async fn leave_sole_owner_is_rejected() {
        let owner = Id::generate();
        let uc = LeaveGroup::new(
            membership(Some(public_group(owner))),
            FixedClock(Timestamp::now()),
        );
        let err = uc
            .execute(LeaveGroupCommand {
                group: Id::generate(),
                user: owner,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Community(CommunityError::SoleOwner)
        ));
    }

    #[tokio::test]
    async fn post_to_public_group_by_owner_publishes() {
        let owner = Id::generate();
        let published = Arc::new(Mutex::new(Vec::new()));
        let uc = PostToGroup::new(
            FakeGroups::with(public_group(owner)),
            FakeGroupPosts {
                published: published.clone(),
            },
            FixedClock(Timestamp::now()),
        );
        let post = uc
            .execute(PostToGroupCommand {
                author: owner,
                group: Id::generate(),
                body: PostBody::parse("трек из подвала").unwrap(),
            })
            .await
            .unwrap();
        assert_eq!(post.author(), owner);
        assert_eq!(published.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn post_to_public_group_by_member_is_rejected() {
        let owner = Id::generate();
        let mut group = public_group(owner);
        let member = Id::generate();
        group.join(member, Timestamp::now()).unwrap();
        let published = Arc::new(Mutex::new(Vec::new()));
        let uc = PostToGroup::new(
            FakeGroups::with(group),
            FakeGroupPosts {
                published: published.clone(),
            },
            FixedClock(Timestamp::now()),
        );
        let err = uc
            .execute(PostToGroupCommand {
                author: member,
                group: Id::generate(),
                body: PostBody::parse("я").unwrap(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Community(CommunityError::NotPermitted)
        ));
        assert_eq!(published.lock().unwrap().len(), 0);
    }

    // --- auth фейки + тесты ---
    #[derive(Clone)]
    struct FakeCredentials {
        stored: Arc<Mutex<Option<Credential>>>,
    }
    impl FakeCredentials {
        fn empty() -> Self {
            Self {
                stored: Arc::new(Mutex::new(None)),
            }
        }
    }
    #[async_trait]
    impl CredentialRepository for FakeCredentials {
        async fn find_by_user(&self, _user: UserId) -> Result<Option<Credential>, RepositoryError> {
            Ok(self.stored.lock().unwrap().clone())
        }
        async fn save(&self, credential: &Credential) -> Result<(), RepositoryError> {
            *self.stored.lock().unwrap() = Some(credential.clone());
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeSessions {
        stored: Arc<Mutex<Vec<Session>>>,
    }
    impl FakeSessions {
        fn empty() -> Self {
            Self {
                stored: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }
    #[async_trait]
    impl SessionRepository for FakeSessions {
        async fn find_by_token(
            &self,
            token: &SessionToken,
        ) -> Result<Option<Session>, RepositoryError> {
            Ok(self
                .stored
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.token() == token)
                .cloned())
        }
        async fn save(&self, session: &Session) -> Result<(), RepositoryError> {
            self.stored.lock().unwrap().push(session.clone());
            Ok(())
        }
        async fn delete(&self, token: &SessionToken) -> Result<(), RepositoryError> {
            self.stored.lock().unwrap().retain(|s| s.token() != token);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeUsers {
        users: Vec<User>,
    }
    impl FakeUsers {
        fn with(users: Vec<User>) -> Self {
            Self { users }
        }
        fn none() -> Self {
            Self { users: Vec::new() }
        }
    }
    #[async_trait]
    impl UserRepository for FakeUsers {
        async fn find_by_id(&self, id: UserId) -> Result<Option<User>, RepositoryError> {
            Ok(self.users.iter().find(|u| u.id() == id).cloned())
        }
        async fn find_by_handle(&self, handle: &Handle) -> Result<Option<User>, RepositoryError> {
            Ok(self.users.iter().find(|u| u.handle() == handle).cloned())
        }
        async fn save(&self, _user: &User) -> Result<(), RepositoryError> {
            Ok(())
        }
    }

    // Детерминированный «хэш»: префикс + сам пароль. Достаточно для проверки потока.
    #[derive(Clone, Copy)]
    struct FakeHasher;
    impl PasswordHasher for FakeHasher {
        fn hash(&self, password: &Password) -> PasswordHash {
            PasswordHash::from_storage(format!("hashed:{}", password.expose()))
        }
        fn verify(&self, password: &Password, hash: &PasswordHash) -> bool {
            hash.as_str() == format!("hashed:{}", password.expose())
        }
    }

    #[derive(Clone)]
    struct FakeTokens(SessionToken);
    impl SessionTokenFactory for FakeTokens {
        fn generate(&self) -> SessionToken {
            self.0.clone()
        }
    }

    fn auth_user() -> User {
        User::register(
            Id::generate(),
            Handle::parse("mc_test").unwrap(),
            UserRole::Member,
            Timestamp::now(),
        )
    }
    fn token() -> SessionToken {
        SessionToken::parse(&"t".repeat(40)).unwrap()
    }
    fn password() -> Password {
        Password::parse("correct horse").unwrap()
    }

    /// Установить пароль и залогиниться; вернуть общий стор сессий и результат.
    async fn logged_in() -> (Timestamp, User, FakeSessions, Authentication) {
        let now = Timestamp::now();
        let user = auth_user();
        let creds = FakeCredentials::empty();
        EstablishCredential::new(creds.clone(), FakeHasher, FixedClock(now))
            .execute(EstablishCredentialCommand {
                user: user.id(),
                password: password(),
            })
            .await
            .unwrap();
        let sessions = FakeSessions::empty();
        let auth = LogIn::new(
            FakeUsers::with(vec![user.clone()]),
            creds,
            sessions.clone(),
            FakeHasher,
            FakeTokens(token()),
            FixedClock(now),
        )
        .execute(LogInCommand {
            handle: user.handle().clone(),
            password: password(),
        })
        .await
        .unwrap();
        (now, user, sessions, auth)
    }

    #[tokio::test]
    async fn login_issues_session_on_correct_password() {
        let (now, _user, sessions, auth) = logged_in().await;
        assert_eq!(auth.token, token());
        assert_eq!(auth.expires_at, now + SESSION_TTL);
        assert_eq!(sessions.stored.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn login_with_wrong_password_is_rejected() {
        let now = Timestamp::now();
        let user = auth_user();
        let creds = FakeCredentials::empty();
        EstablishCredential::new(creds.clone(), FakeHasher, FixedClock(now))
            .execute(EstablishCredentialCommand {
                user: user.id(),
                password: password(),
            })
            .await
            .unwrap();
        let sessions = FakeSessions::empty();
        let err = LogIn::new(
            FakeUsers::with(vec![user.clone()]),
            creds,
            sessions.clone(),
            FakeHasher,
            FakeTokens(token()),
            FixedClock(now),
        )
        .execute(LogInCommand {
            handle: user.handle().clone(),
            password: Password::parse("wrong password").unwrap(),
        })
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Auth(AuthError::InvalidCredentials)
        ));
        assert!(sessions.stored.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn login_with_unknown_handle_is_rejected() {
        let err = LogIn::new(
            FakeUsers::none(),
            FakeCredentials::empty(),
            FakeSessions::empty(),
            FakeHasher,
            FakeTokens(token()),
            FixedClock(Timestamp::now()),
        )
        .execute(LogInCommand {
            handle: Handle::parse("ghost").unwrap(),
            password: password(),
        })
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Auth(AuthError::InvalidCredentials)
        ));
    }

    #[tokio::test]
    async fn authenticate_resolves_current_user() {
        let (now, user, sessions, auth) = logged_in().await;
        let who = Authenticate::new(
            sessions,
            FakeUsers::with(vec![user.clone()]),
            FixedClock(now),
        )
        .execute(AuthenticateCommand { token: auth.token })
        .await
        .unwrap();
        assert_eq!(who.user, user.id());
        assert_eq!(who.handle.as_str(), "mc_test");
        assert_eq!(who.verified, VerifiedStatus::Casual);
    }

    #[tokio::test]
    async fn authenticate_rejects_expired_session() {
        let (now, user, sessions, auth) = logged_in().await;
        let later = now + SESSION_TTL + Duration::days(1);
        let err = Authenticate::new(sessions, FakeUsers::with(vec![user]), FixedClock(later))
            .execute(AuthenticateCommand { token: auth.token })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Auth(AuthError::Unauthenticated)
        ));
    }

    #[tokio::test]
    async fn authenticate_rejects_unknown_token() {
        let err = Authenticate::new(
            FakeSessions::empty(),
            FakeUsers::none(),
            FixedClock(Timestamp::now()),
        )
        .execute(AuthenticateCommand { token: token() })
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Auth(AuthError::Unauthenticated)
        ));
    }

    #[tokio::test]
    async fn logout_invalidates_session() {
        let (now, user, sessions, auth) = logged_in().await;
        LogOut::new(sessions.clone())
            .execute(LogOutCommand {
                token: auth.token.clone(),
            })
            .await
            .unwrap();
        let err = Authenticate::new(sessions, FakeUsers::with(vec![user]), FixedClock(now))
            .execute(AuthenticateCommand { token: auth.token })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Auth(AuthError::Unauthenticated)
        ));
    }

    // --- marketplace фейки + тесты ---
    #[derive(Clone)]
    struct FakeListings {
        saved: Arc<Mutex<Vec<Listing>>>,
    }
    impl FakeListings {
        fn empty() -> Self {
            Self {
                saved: Arc::new(Mutex::new(Vec::new())),
            }
        }
        fn with(listing: Listing) -> Self {
            Self {
                saved: Arc::new(Mutex::new(vec![listing])),
            }
        }
    }
    #[async_trait]
    impl ListingRepository for FakeListings {
        async fn find_by_id(&self, id: ListingId) -> Result<Option<Listing>, RepositoryError> {
            Ok(self
                .saved
                .lock()
                .unwrap()
                .iter()
                .find(|l| l.id() == id)
                .cloned())
        }
        async fn save(&self, listing: &Listing) -> Result<(), RepositoryError> {
            let mut v = self.saved.lock().unwrap();
            v.retain(|l| l.id() != listing.id());
            v.push(listing.clone());
            Ok(())
        }
    }

    fn listing_draft() -> ListingDraft {
        ListingDraft {
            title: ListingTitle::parse("SP-404").unwrap(),
            price: Price::parse(30000).unwrap(),
            description: None,
        }
    }

    fn member(handle: &str) -> User {
        User::register(
            Id::generate(),
            Handle::parse(handle).unwrap(),
            UserRole::Member,
            Timestamp::now(),
        )
    }

    #[tokio::test]
    async fn casual_seller_cannot_list() {
        let seller = member("casual_one");
        let listings = FakeListings::empty();
        let uc = CreateListing::new(
            FakeUsers::with(vec![seller.clone()]),
            listings.clone(),
            FixedClock(Timestamp::now()),
        );
        let err = uc
            .execute(CreateListingCommand {
                seller: seller.id(),
                draft: listing_draft(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Marketplace(MarketplaceError::NotVerified)
        ));
        assert!(listings.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn verified_seller_lists() {
        let mut seller = member("seller_one");
        seller.verify();
        let listings = FakeListings::empty();
        let uc = CreateListing::new(
            FakeUsers::with(vec![seller.clone()]),
            listings.clone(),
            FixedClock(Timestamp::now()),
        );
        let listing = uc
            .execute(CreateListingCommand {
                seller: seller.id(),
                draft: listing_draft(),
            })
            .await
            .unwrap();
        assert_eq!(listing.seller(), seller.id());
        assert_eq!(listings.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn non_seller_cannot_mark_sold() {
        let mut seller = member("seller_two");
        seller.verify();
        let (listing, _) = Listing::list(
            Id::generate(),
            seller.id(),
            seller.verified(),
            listing_draft(),
            Timestamp::now(),
        )
        .unwrap();
        let listing_id = listing.id();
        let uc = MarkListingSold::new(FakeListings::with(listing));
        let err = uc
            .execute(MarkListingSoldCommand {
                listing: listing_id,
                actor: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Marketplace(MarketplaceError::NotSeller)
        ));
    }

    #[tokio::test]
    async fn only_admin_verifies() {
        let actor = member("plain_user");
        let target = member("wannabe");
        let err = VerifyUser::new(FakeUsers::with(vec![actor.clone(), target.clone()]))
            .execute(VerifyUserCommand {
                actor: actor.id(),
                target: target.handle().clone(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::Forbidden(_)));

        let admin = User::register(
            Id::generate(),
            Handle::parse("the_admin").unwrap(),
            UserRole::Admin,
            Timestamp::now(),
        );
        let verified = VerifyUser::new(FakeUsers::with(vec![admin.clone(), target.clone()]))
            .execute(VerifyUserCommand {
                actor: admin.id(),
                target: target.handle().clone(),
            })
            .await
            .unwrap();
        assert!(verified.verified().is_verified());
    }

    // --- verification: заявка → рассмотрение (ADR-0016) ---

    fn admin(handle: &str) -> User {
        User::register(
            Id::generate(),
            Handle::parse(handle).unwrap(),
            UserRole::Admin,
            Timestamp::now(),
        )
    }

    /// Общий стор юзеров и заявок для фейков verification — тест видит итоговое
    /// состояние (статус заявки, верифицирован ли юзер, был ли commit).
    #[derive(Clone)]
    struct FakeVerStore {
        users: Arc<Mutex<Vec<User>>>,
        requests: Arc<Mutex<Vec<VerificationRequest>>>,
        committed: Arc<AtomicBool>,
    }
    impl FakeVerStore {
        fn new(users: Vec<User>, requests: Vec<VerificationRequest>) -> Self {
            Self {
                users: Arc::new(Mutex::new(users)),
                requests: Arc::new(Mutex::new(requests)),
                committed: Arc::new(AtomicBool::new(false)),
            }
        }
        fn request(&self, id: VerificationRequestId) -> VerificationRequest {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id() == id)
                .cloned()
                .unwrap()
        }
        fn user(&self, id: UserId) -> User {
            self.users
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.id() == id)
                .cloned()
                .unwrap()
        }
    }
    #[async_trait]
    impl VerificationRequestRepository for FakeVerStore {
        async fn find_by_id(
            &self,
            id: VerificationRequestId,
        ) -> Result<Option<VerificationRequest>, RepositoryError> {
            Ok(self
                .requests
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id() == id)
                .cloned())
        }
        async fn find_pending_by_requester(
            &self,
            requester: UserId,
        ) -> Result<Option<VerificationRequest>, RepositoryError> {
            Ok(self
                .requests
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.requester() == requester && r.status().is_pending())
                .cloned())
        }
        async fn save(&self, request: &VerificationRequest) -> Result<(), RepositoryError> {
            let mut reqs = self.requests.lock().unwrap();
            if let Some(slot) = reqs.iter_mut().find(|r| r.id() == request.id()) {
                *slot = request.clone();
            } else {
                reqs.push(request.clone());
            }
            Ok(())
        }
    }

    struct FakeDecisionTx(FakeVerStore);
    #[async_trait]
    impl VerificationDecisionTx for FakeDecisionTx {
        async fn find_user(&mut self, id: UserId) -> Result<Option<User>, RepositoryError> {
            Ok(self
                .0
                .users
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.id() == id)
                .cloned())
        }
        async fn lock_request(
            &mut self,
            id: VerificationRequestId,
        ) -> Result<Option<VerificationRequest>, RepositoryError> {
            Ok(self
                .0
                .requests
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id() == id)
                .cloned())
        }
        async fn save_request(
            &mut self,
            request: &VerificationRequest,
        ) -> Result<(), RepositoryError> {
            let mut reqs = self.0.requests.lock().unwrap();
            if let Some(slot) = reqs.iter_mut().find(|r| r.id() == request.id()) {
                *slot = request.clone();
            }
            Ok(())
        }
        async fn save_user(&mut self, user: &User) -> Result<(), RepositoryError> {
            let mut users = self.0.users.lock().unwrap();
            if let Some(slot) = users.iter_mut().find(|u| u.id() == user.id()) {
                *slot = user.clone();
            }
            Ok(())
        }
        async fn commit(&mut self) -> Result<(), RepositoryError> {
            self.0.committed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
    struct FakeDecisionFactory(FakeVerStore);
    #[async_trait]
    impl VerificationDecisionTxFactory for FakeDecisionFactory {
        async fn begin(&self) -> Result<Box<dyn VerificationDecisionTx>, RepositoryError> {
            Ok(Box::new(FakeDecisionTx(self.0.clone())))
        }
    }

    fn pending_for(requester: UserId) -> VerificationRequest {
        VerificationRequest::open(
            VerificationRequestId::generate(),
            requester,
            None,
            Timestamp::now(),
        )
        .0
    }

    #[tokio::test]
    async fn request_opens_pending() {
        let user = member("rapper");
        let store = FakeVerStore::new(vec![], vec![]);
        let req = RequestVerification::new(
            FakeUsers::with(vec![user.clone()]),
            store.clone(),
            FixedClock(Timestamp::now()),
        )
        .execute(RequestVerificationCommand {
            requester: user.id(),
            note: Some(RequestNote::parse("залил три релиза").unwrap()),
        })
        .await
        .unwrap();
        assert!(req.status().is_pending());
        assert_eq!(store.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn already_verified_cannot_request() {
        let mut user = member("verified_guy");
        user.verify();
        let store = FakeVerStore::new(vec![], vec![]);
        let err = RequestVerification::new(
            FakeUsers::with(vec![user.clone()]),
            store.clone(),
            FixedClock(Timestamp::now()),
        )
        .execute(RequestVerificationCommand {
            requester: user.id(),
            note: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(err, ApplicationError::Conflict(_)));
        assert!(store.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn duplicate_pending_request_rejected() {
        let user = member("eager");
        let store = FakeVerStore::new(vec![], vec![pending_for(user.id())]);
        let err = RequestVerification::new(
            FakeUsers::with(vec![user.clone()]),
            store.clone(),
            FixedClock(Timestamp::now()),
        )
        .execute(RequestVerificationCommand {
            requester: user.id(),
            note: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(err, ApplicationError::Conflict(_)));
        assert_eq!(store.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn approve_verifies_user_atomically() {
        let boss = admin("boss");
        let requester = member("hopeful");
        let req = pending_for(requester.id());
        let req_id = req.id();
        let store = FakeVerStore::new(vec![boss.clone(), requester.clone()], vec![req]);
        let event = ApproveVerification::new(
            FakeDecisionFactory(store.clone()),
            FixedClock(Timestamp::now()),
        )
        .execute(ApproveVerificationCommand {
            actor: boss.id(),
            request: req_id,
            reason: None,
        })
        .await
        .unwrap();
        assert_eq!(event.requester, requester.id());
        assert_eq!(store.request(req_id).status(), RequestStatus::Approved);
        assert!(store.user(requester.id()).verified().is_verified());
        assert!(store.committed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn non_admin_cannot_approve() {
        let plain = member("nobody");
        let requester = member("hopeful2");
        let req = pending_for(requester.id());
        let req_id = req.id();
        let store = FakeVerStore::new(vec![plain.clone(), requester.clone()], vec![req]);
        let err = ApproveVerification::new(
            FakeDecisionFactory(store.clone()),
            FixedClock(Timestamp::now()),
        )
        .execute(ApproveVerificationCommand {
            actor: plain.id(),
            request: req_id,
            reason: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(err, ApplicationError::Forbidden(_)));
        assert_eq!(store.request(req_id).status(), RequestStatus::Pending);
        assert!(!store.user(requester.id()).verified().is_verified());
        assert!(!store.committed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn reject_marks_rejected_without_verifying() {
        let boss = admin("boss2");
        let requester = member("hopeful3");
        let req = pending_for(requester.id());
        let req_id = req.id();
        let store = FakeVerStore::new(vec![boss.clone(), requester.clone()], vec![req]);
        RejectVerification::new(
            FakeDecisionFactory(store.clone()),
            FixedClock(Timestamp::now()),
        )
        .execute(RejectVerificationCommand {
            actor: boss.id(),
            request: req_id,
            reason: Some(DecisionReason::parse("аккаунт слишком новый").unwrap()),
        })
        .await
        .unwrap();
        assert_eq!(store.request(req_id).status(), RequestStatus::Rejected);
        assert!(!store.user(requester.id()).verified().is_verified());
    }

    #[tokio::test]
    async fn cannot_approve_already_decided() {
        let boss = admin("boss3");
        let requester = member("hopeful4");
        let mut req = pending_for(requester.id());
        req.reject(boss.id(), None, Timestamp::now()).unwrap();
        let req_id = req.id();
        let store = FakeVerStore::new(vec![boss.clone(), requester.clone()], vec![req]);
        let err = ApproveVerification::new(
            FakeDecisionFactory(store.clone()),
            FixedClock(Timestamp::now()),
        )
        .execute(ApproveVerificationCommand {
            actor: boss.id(),
            request: req_id,
            reason: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Verification(VerificationError::AlreadyDecided)
        ));
    }

    // --- music: релиз/снятие за гейтом верификации (ADR-0017) ---

    struct FakeTracks {
        saved: Arc<Mutex<Vec<Track>>>,
    }
    impl FakeTracks {
        fn empty() -> Self {
            Self {
                saved: Arc::new(Mutex::new(Vec::new())),
            }
        }
        fn with(track: Track) -> Self {
            Self {
                saved: Arc::new(Mutex::new(vec![track])),
            }
        }
    }
    #[async_trait]
    impl TrackRepository for FakeTracks {
        async fn find_by_id(&self, id: TrackId) -> Result<Option<Track>, RepositoryError> {
            Ok(self
                .saved
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id() == id)
                .cloned())
        }
        async fn save(&self, track: &Track) -> Result<(), RepositoryError> {
            let mut v = self.saved.lock().unwrap();
            v.retain(|t| t.id() != track.id());
            v.push(track.clone());
            Ok(())
        }
    }

    fn track_draft() -> TrackDraft {
        TrackDraft {
            title: TrackTitle::parse("Подвал").unwrap(),
            audio: AudioRef::parse("https://audio.example/t.mp3").unwrap(),
            genre: None,
        }
    }

    #[tokio::test]
    async fn casual_artist_cannot_release() {
        let artist = member("casual_mc");
        let tracks = FakeTracks::empty();
        let err = ReleaseTrack::new(
            FakeUsers::with(vec![artist.clone()]),
            tracks,
            FixedClock(Timestamp::now()),
        )
        .execute(ReleaseTrackCommand {
            uploader: artist.id(),
            draft: track_draft(),
        })
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Music(MusicError::NotVerified)
        ));
    }

    #[tokio::test]
    async fn verified_artist_releases() {
        let mut artist = member("real_mc");
        artist.verify();
        let tracks = FakeTracks::empty();
        let track = ReleaseTrack::new(
            FakeUsers::with(vec![artist.clone()]),
            FakeTracks {
                saved: tracks.saved.clone(),
            },
            FixedClock(Timestamp::now()),
        )
        .execute(ReleaseTrackCommand {
            uploader: artist.id(),
            draft: track_draft(),
        })
        .await
        .unwrap();
        assert_eq!(track.uploader(), artist.id());
        assert_eq!(tracks.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn non_uploader_cannot_withdraw() {
        let mut artist = member("mc_two");
        artist.verify();
        let (track, _) = Track::release(
            Id::generate(),
            artist.id(),
            artist.verified(),
            track_draft(),
            Timestamp::now(),
        )
        .unwrap();
        let track_id = track.id();
        let err = WithdrawTrack::new(FakeTracks::with(track))
            .execute(WithdrawTrackCommand {
                track: track_id,
                actor: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Music(MusicError::NotUploader)
        ));
    }

    #[tokio::test]
    async fn uploader_withdraws() {
        let mut artist = member("mc_three");
        artist.verify();
        let (track, _) = Track::release(
            Id::generate(),
            artist.id(),
            artist.verified(),
            track_draft(),
            Timestamp::now(),
        )
        .unwrap();
        let track_id = track.id();
        let tracks = FakeTracks::with(track);
        WithdrawTrack::new(FakeTracks {
            saved: tracks.saved.clone(),
        })
        .execute(WithdrawTrackCommand {
            track: track_id,
            actor: artist.id(),
        })
        .await
        .unwrap();
        assert_eq!(
            tracks.saved.lock().unwrap()[0].status(),
            TrackStatus::Withdrawn
        );
    }

    // --- openapi: выпуск/отзыв/аутентификация ключей (ADR-0018) ---

    #[derive(Clone)]
    struct FakeApiKeys {
        saved: Arc<Mutex<Vec<ApiKey>>>,
    }
    impl FakeApiKeys {
        fn empty() -> Self {
            Self {
                saved: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }
    #[async_trait]
    impl ApiKeyRepository for FakeApiKeys {
        async fn find_by_id(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepositoryError> {
            Ok(self
                .saved
                .lock()
                .unwrap()
                .iter()
                .find(|k| k.id() == id)
                .cloned())
        }
        async fn find_by_hash(&self, hash: &ApiKeyHash) -> Result<Option<ApiKey>, RepositoryError> {
            Ok(self
                .saved
                .lock()
                .unwrap()
                .iter()
                .find(|k| k.hash().as_str() == hash.as_str())
                .cloned())
        }
        async fn save(&self, key: &ApiKey) -> Result<(), RepositoryError> {
            let mut v = self.saved.lock().unwrap();
            v.retain(|k| k.id() != key.id());
            v.push(key.clone());
            Ok(())
        }
    }

    struct FixedApiKeyFactory(ApiKeyToken);
    impl ApiKeyFactory for FixedApiKeyFactory {
        fn generate(&self) -> ApiKeyToken {
            self.0.clone()
        }
    }

    // Детерминированный «хэш»: префикс + сам токен. Достаточно для проверки потока.
    #[derive(Clone, Copy)]
    struct FakeApiKeyHasher;
    impl ApiKeyHasher for FakeApiKeyHasher {
        fn hash(&self, token: &ApiKeyToken) -> ApiKeyHash {
            ApiKeyHash::from_storage(format!("h:{}", token.as_str()))
        }
    }

    fn api_label() -> ApiKeyLabel {
        ApiKeyLabel::parse("ci-bot").unwrap()
    }
    fn api_token() -> ApiKeyToken {
        ApiKeyToken::parse(&format!("bbg_{}", "k".repeat(40))).unwrap()
    }

    #[tokio::test]
    async fn casual_cannot_issue_key() {
        let owner = member("casual_dev");
        let keys = FakeApiKeys::empty();
        let err = IssueApiKey::new(
            FakeUsers::with(vec![owner.clone()]),
            keys.clone(),
            FixedApiKeyFactory(api_token()),
            FakeApiKeyHasher,
            FixedClock(Timestamp::now()),
        )
        .execute(IssueApiKeyCommand {
            owner: owner.id(),
            label: api_label(),
        })
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::OpenApi(OpenApiError::NotVerified)
        ));
        assert!(keys.saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn verified_issues_key_with_token() {
        let mut owner = member("real_dev");
        owner.verify();
        let keys = FakeApiKeys::empty();
        let issued = IssueApiKey::new(
            FakeUsers::with(vec![owner.clone()]),
            keys.clone(),
            FixedApiKeyFactory(api_token()),
            FakeApiKeyHasher,
            FixedClock(Timestamp::now()),
        )
        .execute(IssueApiKeyCommand {
            owner: owner.id(),
            label: api_label(),
        })
        .await
        .unwrap();
        assert_eq!(issued.token.as_str(), api_token().as_str());
        assert_eq!(issued.key.owner(), owner.id());
        // Хранится хэш, не сырой токен.
        assert_eq!(
            keys.saved.lock().unwrap()[0].hash().as_str(),
            "h:bbg_kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk"
        );
    }

    #[tokio::test]
    async fn only_owner_revokes_key() {
        let mut owner = member("dev_one");
        owner.verify();
        let keys = FakeApiKeys::empty();
        let issued = IssueApiKey::new(
            FakeUsers::with(vec![owner.clone()]),
            keys.clone(),
            FixedApiKeyFactory(api_token()),
            FakeApiKeyHasher,
            FixedClock(Timestamp::now()),
        )
        .execute(IssueApiKeyCommand {
            owner: owner.id(),
            label: api_label(),
        })
        .await
        .unwrap();
        let key_id = issued.key.id();
        let err = RevokeApiKey::new(keys.clone())
            .execute(RevokeApiKeyCommand {
                actor: Id::generate(),
                key: key_id,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::OpenApi(OpenApiError::NotOwner)
        ));
        RevokeApiKey::new(keys.clone())
            .execute(RevokeApiKeyCommand {
                actor: owner.id(),
                key: key_id,
            })
            .await
            .unwrap();
        assert_eq!(
            keys.saved.lock().unwrap()[0].status(),
            ApiKeyStatus::Revoked
        );
    }

    #[tokio::test]
    async fn authenticate_resolves_owner_and_rejects_revoked() {
        let mut owner = member("dev_two");
        owner.verify();
        let keys = FakeApiKeys::empty();
        let issued = IssueApiKey::new(
            FakeUsers::with(vec![owner.clone()]),
            keys.clone(),
            FixedApiKeyFactory(api_token()),
            FakeApiKeyHasher,
            FixedClock(Timestamp::now()),
        )
        .execute(IssueApiKeyCommand {
            owner: owner.id(),
            label: api_label(),
        })
        .await
        .unwrap();

        let resolved = AuthenticateApiKey::new(keys.clone(), FakeApiKeyHasher)
            .execute(AuthenticateApiKeyCommand { token: api_token() })
            .await
            .unwrap();
        assert_eq!(resolved, owner.id());

        // неизвестный токен → Unauthenticated
        let unknown = ApiKeyToken::parse(&format!("bbg_{}", "z".repeat(40))).unwrap();
        let err = AuthenticateApiKey::new(keys.clone(), FakeApiKeyHasher)
            .execute(AuthenticateApiKeyCommand { token: unknown })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Auth(AuthError::Unauthenticated)
        ));

        // отозванный ключ → Unauthenticated
        RevokeApiKey::new(keys.clone())
            .execute(RevokeApiKeyCommand {
                actor: owner.id(),
                key: issued.key.id(),
            })
            .await
            .unwrap();
        let err = AuthenticateApiKey::new(keys.clone(), FakeApiKeyHasher)
            .execute(AuthenticateApiKeyCommand { token: api_token() })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Auth(AuthError::Unauthenticated)
        ));
    }
}
