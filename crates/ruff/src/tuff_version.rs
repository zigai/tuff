pub(crate) const VERSION: &str = concat!(
    env!("TUFF_BUILD_VERSION"),
    " (",
    env!("TUFF_BUILD_COMMIT"),
    env!("TUFF_BUILD_DIRTY_SUFFIX"),
    ")"
);
