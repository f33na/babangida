//! Командная сторона CQRS: use-cases записи поверх доменных портов. Каждый
//! проходит через доменные инварианты — лимиты и кулдаун проверяет `domain`
//! ([`Invite::issue`]/[`Invite::accept`]), а не хендлеры.

use babangida_domain::community::{
    Group, GroupId, GroupKind, GroupName, GroupRepository, GroupSlug, MemberJoined, MemberLeft,
    MemberRoleChanged, MembershipRole,
};
use babangida_domain::content::{Post, PostBody, PostRepository};
use babangida_domain::identity::{
    Handle, Invite, InviteAccepted, InviteCode, InviteId, InviteIssued, InviteRepository,
    IssuanceContext, User, UserId, UserRole,
};
use babangida_domain::messaging::{
    Conversation, ConversationId, ConversationRepository, MessageBody, MessageId,
    MessageRepository, MessageSent,
};
use babangida_domain::social::{DisplayName, Profile, Subculture};
use babangida_shared::Id;

use crate::{
    ApplicationError, Clock, GroupMembershipTxFactory, InviteCodeFactory, IssueInviteTxFactory,
    RegistrationTxFactory,
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

/// Зарегистрироваться по инвайту: создать юзера и профиль, пометить инвайт принятым.
pub struct RegisterCommand {
    pub code: InviteCode,
    pub handle: Handle,
    pub display_name: DisplayName,
    pub subculture: Subculture,
}

/// Use-case регистрации. Атомарен: блокировка активного инвайта, создание юзера/профиля
/// и пометка инвайта — в одной транзакции (порт [`RegistrationTxFactory`]).
pub struct Register<T, C> {
    tx_factory: T,
    clock: C,
}

impl<T, C> Register<T, C>
where
    T: RegistrationTxFactory,
    C: Clock,
{
    pub fn new(tx_factory: T, clock: C) -> Self {
        Self { tx_factory, clock }
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use babangida_domain::RepositoryError;
    use babangida_domain::community::CommunityError;
    use babangida_domain::identity::{InviteError, InviteQuota};
    use babangida_domain::messaging::{Message, MessagingError};
    use babangida_shared::{Duration, Timestamp};

    use super::*;
    use crate::{GroupMembershipTx, GroupMembershipTxFactory, InviterIssuanceState, IssueInviteTx};

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
}
