#![deny(missing_docs)]
#![allow(clippy::result_large_err)]

//! # axotag
//!
//! This library contains tag-parsing code for use with cargo-dist.

use std::collections::BTreeMap;

use axoproject::{PackageIdx, PackageInfo};
use errors::{TagError, TagResult};
use semver::Version;

pub mod errors;

/// details on what we're announcing (partially computed)
pub struct PartialAnnouncementTag {
    /// The full tag
    pub tag: Option<String>,
    /// The version we're announcing (if doing a unified version announcement)
    pub version: Option<Version>,
    /// The package we're announcing (if doing a single-package announcement)
    pub package: Option<PackageIdx>,
    /// whether we're prereleasing
    pub prerelease: bool,
}

/// Do the actual parsing logic for a tag
///
/// If `tag` is None, then we had no --tag to parse, and need to do inference.
/// The return value is then essentially a default/empty PartialAnnouncementTag
/// which later passes will fill in.
pub fn parse_tag(
    packages: &BTreeMap<PackageIdx, &PackageInfo>,
    tag: Option<&str>,
) -> TagResult<PartialAnnouncementTag> {
    // First thing's first: if they gave us an announcement tag then we should try to parse it
    let mut announcing_package = None;
    let mut announcing_version = None;
    let mut announcing_prerelease = false;
    let announcement_tag = tag.map(|t| t.to_owned());
    if let Some(tag) = &announcement_tag {
        let mut tag_suffix;
        // Check if we're using `/`'s to delimit things
        if let Some((prefix, suffix)) = tag.rsplit_once('/') {
            // We're at least in "blah/v1.0.0" format
            let maybe_package = if let Some((_prefix, package)) = prefix.rsplit_once('/') {
                package
            } else {
                // There's only one `/`, assume the whole prefix could be a package name
                prefix
            };
            // Check if this is "blah/blah/some-package/v1.0.0" format by checking if the last slash-delimited
            // component is exactly a package name (strip_prefix produces empty string)
            if let Some((package, "")) = strip_prefix_package(maybe_package, packages) {
                announcing_package = Some(package);
            }
            tag_suffix = suffix;
        } else {
            tag_suffix = tag;
        };

        // If we don't have an announcing_package yet, check if this is "some-package-v1.0.0" format
        if announcing_package.is_none() {
            if let Some((package, suffix)) = strip_prefix_package(tag_suffix, packages) {
                // Must be followed by a dash to be accepted
                if let Some(suffix) = suffix.strip_prefix('-') {
                    tag_suffix = suffix;
                    announcing_package = Some(package);
                }
            }
        }

        // At this point, assuming the input is valid, tag_suffix should just be the version
        // component with an optional "v" prefix, so strip that "v"
        if let Some(suffix) = tag_suffix.strip_prefix('v') {
            tag_suffix = suffix;
        }

        // Now parse the version out
        match tag_suffix.parse::<Version>() {
            Ok(version) => {
                // Register whether we're announcing a prerelease
                announcing_prerelease = !version.pre.is_empty();

                // If there's an announcing package, validate that the version matches
                if let Some(pkg_idx) = announcing_package {
                    if let Some(package) = packages.get(&pkg_idx) {
                        if let Some(real_version) = &package.version {
                            if real_version.cargo() != &version {
                                return Err(TagError::ContradictoryTagVersion {
                                    tag: tag.clone(),
                                    package_name: package.name.clone(),
                                    tag_version: version,
                                    real_version: real_version.clone(),
                                });
                            }
                        }
                    }
                } else {
                    // We had no announcing_package, so looks like we're doing a unified release.
                    // Set this value to indicate that.
                    announcing_version = Some(version);
                }
            }
            Err(e) => {
                return Err(TagError::TagVersionParse {
                    tag: tag.clone(),
                    details: e,
                })
            }
        }

        // If none of the approaches work, refuse to proceed
        if announcing_package.is_none() && announcing_version.is_none() {
            return Err(TagError::NoTagMatch { tag: tag.clone() });
        }
    }
    Ok(PartialAnnouncementTag {
        tag: announcement_tag,
        prerelease: announcing_prerelease,
        version: announcing_version,
        package: announcing_package,
    })
}

/// Try to strip-prefix a package name from the given input, preferring whichever one is longest
/// (to disambiguate situations where you have `my-app` and `my-app-helper`).
///
/// If a match is found, then the return value is:
/// * the idx of the package
/// * the rest of the input
fn strip_prefix_package<'a>(
    input: &'a str,
    packages: &BTreeMap<PackageIdx, &PackageInfo>,
) -> Option<(PackageIdx, &'a str)> {
    let mut result: Option<(PackageIdx, &'a str)> = None;
    for (pkg_id, package) in packages {
        if let Some(rest) = input.strip_prefix(&package.name) {
            if let Some((_, best)) = result {
                if best.len() <= rest.len() {
                    continue;
                }
            }
            result = Some((*pkg_id, rest))
        }
    }
    result
}
