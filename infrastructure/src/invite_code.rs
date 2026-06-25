use babangida_application::InviteCodeFactory;
use babangida_domain::identity::InviteCode;
use uuid::Uuid;

/// Случайный код приглашения. Источник энтропии — UUID v4 (32 hex-символа,
/// ASCII-буквенно-цифровые), домен его только валидирует.
pub struct RandomInviteCodeFactory;

impl InviteCodeFactory for RandomInviteCodeFactory {
    fn generate(&self) -> InviteCode {
        let raw = Uuid::new_v4().simple().to_string().to_uppercase();
        InviteCode::parse(&raw).expect("32 hex-символа — валидный InviteCode")
    }
}
