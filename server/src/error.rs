// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
/// Upper limit of profiles (a family). The migration enforces the same number.
pub const MAX_PROFILES: u64 = 150;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("profile limit reached (max {MAX_PROFILES})")]
    ProfileLimitReached,
    #[error("the last admin cannot be removed or demoted")]
    LastAdmin,
    #[error("the user has reached the limit of devices")]
    DeviceLimitReached,
    #[error("a child needs an age limit and a weekly planner")]
    ChildRequiresAgeAndSchedule,
    #[error("a book can only be released with an age rating")]
    ReleaseRequiresAge,
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("database connection: {0}")]
    Sqlx(#[from] sea_orm::sqlx::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("audio tags in {file}: {message}")]
    Tags { file: String, message: String },
    #[error("the import is locked until the release criteria are defined")]
    ImportLocked,
    #[error("the release criteria are not defined: import roots and a rating rule are needed")]
    ImportCriteriaMissing,
    #[error("not a usable import folder: {0}")]
    InvalidImportRoot(String),
    #[error("the pairing code is invalid, used or expired")]
    EnrollmentInvalid,
    #[error("a device name has 1 to 80 characters")]
    InvalidDeviceName,
    #[error("not a WireGuard public key (44 characters of base64)")]
    InvalidPublicKey,
    #[error("an administrator exists already")]
    AdminExists,
    #[error("a PIN has 4 to 12 digits; a password has 8 to 64 characters with letters and digits")]
    InvalidPin,
    #[error("a picture must be a PNG, JPEG or WebP file")]
    AvatarType,
    #[error("a picture may be at most {} KiB", crate::profile::MAX_AVATAR_BYTES / 1024)]
    AvatarSize,
    #[error("a nickname has 1 to 80 characters, first and last name at most 80")]
    InvalidName,
    #[error("not a usable birthday (a date in the past, at most 120 years ago)")]
    InvalidBirthDate,
    #[error("a blurb has at most {} characters", crate::store::MAX_DESCRIPTION_CHARS)]
    DescriptionTooLong,
    #[error("the current PIN is wrong")]
    WrongPin,
    #[error("too many wrong PINs; try again later")]
    PinLocked,
    #[error("template: {0}")]
    Template(String),
    #[error("write queue stopped")]
    WriterStopped,
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Turns the messages of the database triggers into typed errors.
    pub(crate) fn from_db(e: sea_orm::DbErr) -> Error {
        let message = e.to_string();
        if message.contains("profile_limit_reached") {
            Error::ProfileLimitReached
        } else if message.contains("last_admin") {
            Error::LastAdmin
        } else if message.contains("UNIQUE constraint failed: device.wg_pubkey") {
            Error::InvalidPublicKey
        } else if message.contains("device_limit_reached") {
            Error::DeviceLimitReached
        } else if message.contains("child_requires_age_and_schedule") {
            Error::ChildRequiresAgeAndSchedule
        } else if message.contains("release_requires_age") {
            Error::ReleaseRequiresAge
        } else {
            Error::Db(e)
        }
    }
}
