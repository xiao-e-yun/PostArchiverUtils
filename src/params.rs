use std::{fmt, str::FromStr};

/// Represents the strategy for importing data into the archive.
///
/// Implements [`clap::ValueEnum`], so `--help` lists the values with their descriptions.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum Strategy {
    /// Only fetch posts newer than the last run, and skip posts already in the archive
    #[default]
    Increment,
    /// Fetch everything again and re-import posts already in the archive
    Full,
}

impl FromStr for Strategy {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "increment" => Ok(Strategy::Increment),
            "full" => Ok(Strategy::Full),
            _ => Err(format!("Invalid strategy: {}", s)),
        }
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Self::Increment => "increment",
            Self::Full => "full",
        })
    }
}
