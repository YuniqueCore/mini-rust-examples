use std::{str::FromStr,ops::{Deref,DerefMut}};

use crate::impl_deref_mut;

#[derive(Debug)]
pub struct RoutePath(String);

impl FromStr for RoutePath {
    type Err=core::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RoutePath(String::from_str(s)?))
    }
}

impl_deref_mut!(RoutePath(String));