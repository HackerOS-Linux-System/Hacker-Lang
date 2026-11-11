pub mod prepare;
pub mod profiles;
pub mod registry;
pub mod version;

pub use prepare::{
    prepare, Declared, Level, Prepared, SyntaxDiag, SyntaxError, SyntaxInfo, Take,
};
pub use profiles::{find_profile, list_profiles, profile_dirs, Profile};
pub use registry::{
    find_feature, gen_for_version, resolve_version, version_for_legacy_gen, Feature, ResolvedVersion,
    DEFAULT_GEN, FEATURES, GEN1_FINAL, GEN2_START, LATEST_SYNTAX, MAX_GEN, OLDEST_SYNTAX,
    TOOL_VERSION,
};
pub use version::{ToolVersion, Version};

#[cfg(test)]
mod tests;
