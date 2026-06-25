//! Интеграционные тесты выдачи инвайта против реальной Postgres (ADR-0009/0011).
//! Запускаются под `nix develop` (DATABASE_URL задан, БД поднята); без БД — скип.
//! Все сценарии в одном тесте: общая БД, чтобы не гонять `TRUNCATE` параллельно.

use std::sync::Arc;

use babangida_application::command::{IssueInvite, IssueInviteCommand};
use babangida_domain::identity::{Handle, User, UserId, UserRepository, UserRole};
use babangida_infrastructure::{
    Db, PgIssueInviteTxFactory, PgUserRepository, RandomInviteCodeFactory, SystemClock, connect,
    run_migrations,
};
use babangida_shared::{Id, Timestamp};

async fn setup() -> Option<Db> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let db = connect(&url).await.ok()?;
    run_migrations(&db).await.expect("миграции");
    sqlx::query("TRUNCATE users, invites RESTART IDENTITY CASCADE")
        .execute(&db)
        .await
        .expect("truncate");
    Some(db)
}

async fn active_count(db: &Db, inviter: UserId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM invites WHERE inviter_id = $1 AND status = 'active'")
        .bind(inviter.as_uuid())
        .fetch_one(db)
        .await
        .expect("count")
}

async fn make_user(db: &Db, handle: &str, role: UserRole) -> UserId {
    let user = User::register(
        Id::generate(),
        Handle::parse(handle).unwrap(),
        role,
        Timestamp::now(),
    );
    PgUserRepository::new(db.clone())
        .save(&user)
        .await
        .expect("save user");
    user.id()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invite_issuance_holds_against_postgres() {
    let Some(db) = setup().await else {
        eprintln!("SKIP invite_issuance_holds_against_postgres: DATABASE_URL не задан");
        return;
    };

    let uc = Arc::new(IssueInvite::new(
        PgIssueInviteTxFactory::new(db.clone()),
        SystemClock,
        RandomInviteCodeFactory,
    ));

    // 1) member: первая выдача ок, немедленная вторая — кулдаун.
    let member = make_user(&db, "memberone", UserRole::Member).await;
    uc.execute(IssueInviteCommand { inviter: member })
        .await
        .expect("первый инвайт");
    let second = uc.execute(IssueInviteCommand { inviter: member }).await;
    assert!(
        second.is_err(),
        "вторая выдача в пределах кулдауна должна быть отклонена"
    );
    assert_eq!(active_count(&db, member).await, 1);

    // 2) admin: без лимита и кулдауна — три подряд проходят.
    let admin = make_user(&db, "adminboss", UserRole::Admin).await;
    for _ in 0..3 {
        uc.execute(IssueInviteCommand { inviter: admin })
            .await
            .expect("админ без лимита");
    }
    assert_eq!(active_count(&db, admin).await, 3);

    // 3) конкуренция: три параллельные выдачи свежего member сериализуются
    //    блокировкой строки инвайтера (ADR-0011) — успешна ровно одна, остальные
    //    ловят кулдаун. Без блокировки гонка могла бы вставить несколько.
    let racer = make_user(&db, "racerman", UserRole::Member).await;
    let mut handles = Vec::new();
    for _ in 0..3 {
        let uc = Arc::clone(&uc);
        handles.push(tokio::spawn(async move {
            uc.execute(IssueInviteCommand { inviter: racer }).await
        }));
    }
    let mut oks = 0;
    for h in handles {
        if h.await.expect("join").is_ok() {
            oks += 1;
        }
    }
    assert_eq!(
        oks, 1,
        "под блокировкой проходит ровно одна параллельная выдача"
    );
    assert_eq!(
        active_count(&db, racer).await,
        1,
        "квота/кулдаун не обойдены гонкой"
    );
}
