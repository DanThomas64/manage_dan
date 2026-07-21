//! Backend implementation for todo persistence.
//!
//! `todo::lib` dispatches every public CRUD/read function to this module,
//! which owns the mapping between [`crate::models::TodoItem`] and its `nb`
//! file representation.

pub mod nb;
