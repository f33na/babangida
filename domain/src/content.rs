//! Контекст content: посты. Чтение ленты — это read-модель CQRS в `application`
//! (ADR-0004), здесь только запись.

use async_trait::async_trait;
use babangida_shared::{Id, Timestamp};

use crate::RepositoryError;
use crate::identity::UserId;

/// Фантомный маркер для типизированного [`PostId`].
pub enum PostMarker {}
/// Идентификатор поста.
pub type PostId = Id<PostMarker>;

/// Тело поста. 1..=5000 символов после обрезки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostBody(String);

impl PostBody {
    /// Максимальная длина.
    pub const MAX_LEN: usize = 5000;

    /// Распарсить тело поста.
    ///
    /// # Errors
    /// [`PostBodyError`], если пусто или длиннее [`PostBody::MAX_LEN`].
    pub fn parse(input: &str) -> Result<Self, PostBodyError> {
        let body = input.trim();
        if body.is_empty() {
            return Err(PostBodyError::Empty);
        }
        let len = body.chars().count();
        if len > Self::MAX_LEN {
            return Err(PostBodyError::TooLong { len });
        }
        Ok(Self(body.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`PostBody`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PostBodyError {
    #[error("пост пустой")]
    Empty,
    #[error("пост слишком длинный: {len} символов")]
    TooLong { len: usize },
}

/// Пост — корень агрегата content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    id: PostId,
    author: UserId,
    body: PostBody,
    created_at: Timestamp,
}

impl Post {
    /// Создать пост. Идентификатор и время приходят с границы.
    #[must_use]
    pub fn create(id: PostId, author: UserId, body: PostBody, now: Timestamp) -> Self {
        Self {
            id,
            author,
            body,
            created_at: now,
        }
    }

    #[must_use]
    pub const fn id(&self) -> PostId {
        self.id
    }

    #[must_use]
    pub const fn author(&self) -> UserId {
        self.author
    }

    #[must_use]
    pub fn body(&self) -> &PostBody {
        &self.body
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Хранилище постов (порт записи; реализация — в `infrastructure`).
#[async_trait]
pub trait PostRepository: Send + Sync {
    async fn find_by_id(&self, id: PostId) -> Result<Option<Post>, RepositoryError>;
    async fn save(&self, post: &Post) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_body_validates() {
        assert_eq!(PostBody::parse("  йоу  ").unwrap().as_str(), "йоу");
        assert_eq!(PostBody::parse("   "), Err(PostBodyError::Empty));
        assert!(matches!(
            PostBody::parse(&"a".repeat(5001)),
            Err(PostBodyError::TooLong { len: 5001 })
        ));
    }

    #[test]
    fn post_carries_author_and_body() {
        let author = Id::generate();
        let post = Post::create(
            Id::generate(),
            author,
            PostBody::parse("первый трек").unwrap(),
            Timestamp::now(),
        );
        assert_eq!(post.author(), author);
        assert_eq!(post.body().as_str(), "первый трек");
    }
}
