//! Destructive filesystem actions: deleting cache and archiving to external.
//!
//! These are the only place files get removed or moved. Every operation is
//! byte-accurate and produces a manifest so archiving is reversible.

pub mod cleaner;
pub mod archiver;
pub mod undo_cleaner;

pub use cleaner::{clean_folder, clean_file, CleanOutcome};
pub use archiver::{run_archive, undo_archive, ArchiveOutcome, UndoOutcome};
pub use undo_cleaner::{
    clean_folder_to_recycle_bin,
    clean_file_to_recycle_bin,
    restore_clean_session,
    purge_clean_session,
    CleanSession,
    RecycledEntry,
    SESSION_MANIFEST_NAME,
};
