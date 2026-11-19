#[doc(hidden)]
pub use {console, indicatif};

#[macro_export]
macro_rules! display_metadata {
    ($name: literal { $(  $key: literal: $value: expr  ),* $(,)? }) => {
        println!("# {} #", $crate::display::console::style($name).bold().green());
        println!("==================================");
        $( println!("{} {}", $key, $crate::display::console::style($value).bold().green());)*
        println!("==================================");
    };
}

#[macro_export]
macro_rules! define_progress {
    ($name: ident [$( $pb: ident ),*]) => {
        #[derive(Debug, Clone)]
        pub struct $name {
            $( pub $pb: $crate::display::indicatif::ProgressBar, )*
        }

        impl $name {
            pub fn new(multi: &$crate::display::indicatif::MultiProgress) -> Self {
                Self {
                    $( $pb: multi.add($crate::display::indicatif::ProgressBar::new(0))
                        .with_style($crate::display::progress_style())
                        .with_prefix(stringify!($pb)), )*
                }
            }
        }
    };
}

#[doc(hidden)]
pub fn progress_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:.bold.dim} {wide_bar:.cyan/blue} {pos:>3}/{len:3}")
        .unwrap()
        .progress_chars("#>-")
}

pub use display_metadata;
use indicatif::ProgressStyle;
