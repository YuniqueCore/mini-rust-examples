use crate::dns::Host;
use crate::error::Result;
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::Read,
    path::PathBuf,
};

#[cfg(windows)]
const DEFAULT_HOSTS_PATH: &str = r"C:\Windows\System32\drivers\etc\hosts";
#[cfg(not(windows))]
const DEFAULT_HOSTS_PATH: &str = "/etc/hosts";

pub struct LocalDnsResolver {
    mapping: HashSet<Host>,
}

impl LocalDnsResolver {
    fn parse_host(host_path: &PathBuf) -> Result<HashSet<Host>> {
        let mut hosts = HashSet::new();
        let content = OpenOptions::new().read(true).open(host_path)?;
        content.re
    }
    pub fn new(host_path: impl Into<PathBuf>) -> Self {
        if host_path.into().exists() {}
    }
}
