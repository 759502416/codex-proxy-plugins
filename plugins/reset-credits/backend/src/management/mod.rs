mod accounts;
mod auto;
mod credits;
mod registration;
mod reset;
mod response;
mod router;
mod validation;

pub(crate) use registration::registration;
pub(crate) use router::handle;
