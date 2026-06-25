//! Интеграционный тест атомарности членства против реальной Postgres
//! (ADR-0009/0012). Доказывает: инвариант «всегда есть владелец» держится под
//! конкуренцией — блокировка строки группы сериализует параллельные выходы.
//! Без DATABASE_URL — скип.

use std::sync::Arc;

use babangida_application::command::{
    FoundGroup, FoundGroupCommand, JoinGroup, JoinGroupCommand, LeaveGroup, LeaveGroupCommand,
    SetMemberRole, SetMemberRoleCommand,
};
use babangida_domain::community::{GroupId, GroupKind, GroupName, GroupSlug, MembershipRole};
use babangida_domain::identity::{Handle, User, UserId, UserRepository, UserRole};
use babangida_infrastructure::{
    Db, PgGroupMembershipTxFactory, PgGroupRepository, PgUserRepository, SystemClock, connect,
    run_migrations,
};
use babangida_shared::{Id, Timestamp};

async fn setup() -> Option<Db> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let db = connect(&url).await.ok()?;
    run_migrations(&db).await.expect("миграции");
    sqlx::query("TRUNCATE users, groups RESTART IDENTITY CASCADE")
        .execute(&db)
        .await
        .expect("truncate");
    Some(db)
}

async fn make_user(db: &Db, handle: &str) -> UserId {
    let user = User::register(
        Id::generate(),
        Handle::parse(handle).unwrap(),
        UserRole::Member,
        Timestamp::now(),
    );
    PgUserRepository::new(db.clone())
        .save(&user)
        .await
        .expect("save user");
    user.id()
}

async fn owner_count(db: &Db, group: GroupId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM group_members WHERE group_id = $1 AND role = 'owner'")
        .bind(group.as_uuid())
        .fetch_one(db)
        .await
        .expect("owner count")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn group_keeps_an_owner_under_concurrent_leaves() {
    let Some(db) = setup().await else {
        eprintln!("SKIP group_keeps_an_owner_under_concurrent_leaves: DATABASE_URL не задан");
        return;
    };

    let alpha = make_user(&db, "alphaown").await;
    let beta = make_user(&db, "betaown").await;
    let clock = SystemClock;

    // Паблик с владельцем alpha; beta вступает и повышается до владельца — теперь
    // владельцев двое.
    let group = FoundGroup::new(PgGroupRepository::new(db.clone()), clock)
        .execute(FoundGroupCommand {
            founder: alpha,
            slug: GroupSlug::parse("crew").unwrap(),
            name: GroupName::parse("Crew").unwrap(),
            kind: GroupKind::Public,
        })
        .await
        .expect("основание");
    let gid = group.id();

    JoinGroup::new(PgGroupMembershipTxFactory::new(db.clone()), clock)
        .execute(JoinGroupCommand {
            group: gid,
            user: beta,
        })
        .await
        .expect("вступление beta");
    SetMemberRole::new(PgGroupMembershipTxFactory::new(db.clone()), clock)
        .execute(SetMemberRoleCommand {
            group: gid,
            actor: alpha,
            target: beta,
            role: MembershipRole::Owner,
        })
        .await
        .expect("повышение beta до владельца");

    // Оба владельца выходят одновременно. Блокировка строки группы (ADR-0012)
    // сериализует выходы: первый уходит, второй упирается в инвариант «последний
    // владелец». Успешен ровно один.
    let leave = Arc::new(LeaveGroup::new(
        PgGroupMembershipTxFactory::new(db.clone()),
        clock,
    ));
    let mut handles = Vec::new();
    for user in [alpha, beta] {
        let leave = Arc::clone(&leave);
        handles.push(tokio::spawn(async move {
            leave.execute(LeaveGroupCommand { group: gid, user }).await
        }));
    }
    let mut oks = 0;
    for h in handles {
        if h.await.expect("join").is_ok() {
            oks += 1;
        }
    }

    assert_eq!(oks, 1, "под блокировкой выходит ровно один владелец");
    assert_eq!(
        owner_count(&db, gid).await,
        1,
        "у группы остаётся владелец — инвариант не обойдён гонкой"
    );
}
