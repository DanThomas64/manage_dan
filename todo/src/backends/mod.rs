//! Backend implementations for todo persistence.
//!
//! `todo::lib` dispatches every public CRUD/read function to one of these
//! based on the configured [`crate::BackendKind`]. Each backend module owns
//! its own mapping between [`crate::models::TodoItem`] and its storage
//! representation.

pub mod nb;
pub mod vikunja;
