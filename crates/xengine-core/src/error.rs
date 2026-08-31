//! Core error types for the ECS world and scheduling.

use std::fmt;

/// Errors produced by [`crate::world::World`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldError {
    /// The entity handle's generation does not match the live entity (stale).
    StaleEntity,
    /// The entity already has the component type being added.
    InsertAlreadyExists(&'static str),
    /// The component type is not registered in the registry.
    ComponentNotRegistered(&'static str),
    /// The component type was already registered.
    DuplicateRegistration(&'static str),
    /// The resource is not registered / not present.
    ResourceNotFound(&'static str),
    /// A scheduling operation failed because of an unsafe borrow conflict.
    BorrowConflict(&'static str),
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleEntity => write!(f, "stale entity handle (generation mismatch)"),
            Self::InsertAlreadyExists(name) => {
                write!(f, "component '{name}' already exists on the entity")
            }
            Self::ComponentNotRegistered(name) => write!(f, "component '{name}' is not registered"),
            Self::DuplicateRegistration(name) => write!(f, "component '{name}' is already registered"),
            Self::ResourceNotFound(name) => write!(f, "resource '{name}' is not present"),
            Self::BorrowConflict(name) => write!(f, "borrow conflict on '{name}'"),
        }
    }
}

impl std::error::Error for WorldError {}

/// Result alias for world operations.
pub type WorldResult<T> = Result<T, WorldError>;
