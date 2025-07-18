use std::fmt::Display;

use console::style;
use log::info;

pub fn display_metadata(name: &str, metadata: &[(impl AsRef<str>,impl Display)]) {
    info!("# {} #", style(name).bold().green());
    info!("==================================");
    for (key, value) in metadata {
        info!("{} {}", key.as_ref(),style(value).bold().green());
    }
    info!("==================================");
}
