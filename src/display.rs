#[macro_export]
macro_rules! display_metadata {
    ($name: literal { $(  $key: literal: $value: expr  ),* $(,)? }) => {
        log::info!("# {} #", console::style($name).bold().green());
        log::info!("==================================");
        $(
            log::info!("{} {}", $key, console::style($value).bold().green());
        )*
        log::info!("==================================");
    };
}

pub use display_metadata;
