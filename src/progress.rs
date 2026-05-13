use std::{cell::RefCell, collections::HashMap};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle, WeakProgressBar};

pub struct ProgressSet {
    pub multi: MultiProgress,
    pub map: RefCell<HashMap<&'static str, WeakProgressBar>>,
    pub disabled: &'static[&'static str],
}

impl ProgressSet {
    pub fn new(multi: MultiProgress) -> Self {
        Self {
            multi: multi,
            map: Default::default(),
            disabled: &[],
        }
    }

    pub fn disabled(mut self, prefixes: &'static[&'static str]) -> Self {
        self.disabled = prefixes;
        self
    }

    pub fn add(&self, prefix: &'static str) -> ProgressBar {
        if self.disabled.contains(&prefix) {
            return ProgressBar::hidden();
        }

        let mut map = self.map.borrow_mut();

        if let Some(weak) = map.get(prefix)
            && let Some(bar) = weak.upgrade()
        {
            return bar;
        }

        let pb = self.multi.add(
            ProgressBar::new(0)
                .with_style(Self::style())
                .with_prefix(format!("[{prefix}]")),
        );

        map.insert(prefix, pb.downgrade());
        pb
    }

    fn style() -> ProgressStyle {
        ProgressStyle::with_template("{prefix:.bold.dim} {wide_bar:.cyan/blue} {pos:>3}/{len:3}")
            .unwrap()
            .progress_chars("#>-")
    }
}
