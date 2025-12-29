use std::{str::FromStr,ops::{Deref,DerefMut}};

use crate::impl_deref_mut;

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct RoutePath(String);

impl FromStr for RoutePath {
    type Err=core::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RoutePath(String::from_str(s)?))
    }
}

impl From<&str> for RoutePath {
    fn from(value: &str) -> Self {
        RoutePath::from_str(value).unwrap()
    }
}

impl_deref_mut!(RoutePath(String));