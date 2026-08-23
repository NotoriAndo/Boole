use boole_core::{
    h_protocol, CanonicalPackage, PackageFile, PackageRoot, PackageSidecarError,
    MAX_PACKAGE_CANONICAL_BYTES, MAX_PACKAGE_FILES, PACKAGE_SIDECAR_ROOT_DOMAIN,
    PACKAGE_SIDECAR_SCHEMA,
};

#[test]
fn canonical_package_is_sorted_and_has_a_pinned_big_endian_layout() {
    let package = CanonicalPackage::new(vec![
        PackageFile::new(b"src/z.rs", b"z"),
        PackageFile::new(b"README.md", b"hi"),
    ])
    .expect("valid package");

    let mut expected = Vec::new();
    expected.extend_from_slice(&(PACKAGE_SIDECAR_SCHEMA.len() as u32).to_be_bytes());
    expected.extend_from_slice(PACKAGE_SIDECAR_SCHEMA.as_bytes());
    expected.extend_from_slice(&2u32.to_be_bytes());
    expected.extend_from_slice(&9u32.to_be_bytes());
    expected.extend_from_slice(b"README.md");
    expected.extend_from_slice(&2u64.to_be_bytes());
    expected.extend_from_slice(b"hi");
    expected.extend_from_slice(&8u32.to_be_bytes());
    expected.extend_from_slice(b"src/z.rs");
    expected.extend_from_slice(&1u64.to_be_bytes());
    expected.extend_from_slice(b"z");

    assert_eq!(package.canonical_bytes(), expected);
    assert_eq!(package.size_bytes(), expected.len());
    let _: PackageRoot = package.root();

    let reversed = CanonicalPackage::new(vec![
        PackageFile::new(b"README.md", b"hi"),
        PackageFile::new(b"src/z.rs", b"z"),
    ])
    .expect("same files in a different input order");
    assert_eq!(reversed.canonical_bytes(), package.canonical_bytes());
    assert_eq!(reversed.root(), package.root());
}

#[test]
fn package_rejects_a_non_utf8_path() {
    let error = CanonicalPackage::new(vec![PackageFile::new([0xff], b"bytes")])
        .expect_err("paths are a portable UTF-8 contract");
    assert_eq!(error, PackageSidecarError::PathNotUtf8);
}

#[test]
fn package_rejects_unsafe_or_non_relative_paths() {
    let cases: &[(&[u8], PackageSidecarError)] = &[
        (b"", PackageSidecarError::EmptyPath),
        (b"/etc/passwd", PackageSidecarError::AbsolutePath),
        (b"C:/Windows/file", PackageSidecarError::AbsolutePath),
        (b"C:relative-file", PackageSidecarError::AbsolutePath),
        (b".", PackageSidecarError::DotPathComponent),
        (b"src/./lib.rs", PackageSidecarError::DotPathComponent),
        (b"src/../secret", PackageSidecarError::DotPathComponent),
        (b"src//lib.rs", PackageSidecarError::EmptyPathComponent),
        (b"src/", PackageSidecarError::EmptyPathComponent),
        (b"src\\lib.rs", PackageSidecarError::BackslashInPath),
        (b"src/zero\0byte", PackageSidecarError::NulInPath),
    ];

    for (path, expected) in cases {
        let error = CanonicalPackage::new(vec![PackageFile::new(path, b"bytes")])
            .expect_err("unsafe paths must be rejected");
        assert_eq!(&error, expected, "path={path:?}");
    }
}

#[test]
fn package_rejects_duplicate_paths_after_sorting() {
    let error = CanonicalPackage::new(vec![
        PackageFile::new(b"src/lib.rs", b"first"),
        PackageFile::new(b"README.md", b"ok"),
        PackageFile::new(b"src/lib.rs", b"second"),
    ])
    .expect_err("one canonical path must name at most one file");
    assert_eq!(error, PackageSidecarError::DuplicatePath);
}

#[test]
fn package_file_count_bound_is_inclusive_at_4096() {
    assert_eq!(MAX_PACKAGE_FILES, 4096);
    let files = (0..MAX_PACKAGE_FILES)
        .map(|index| PackageFile::new(format!("f/{index:04}"), []))
        .collect();
    CanonicalPackage::new(files).expect("the frozen file-count ceiling is inclusive");

    let files = (0..=MAX_PACKAGE_FILES)
        .map(|index| PackageFile::new(format!("f/{index:04}"), []))
        .collect();
    assert_eq!(
        CanonicalPackage::new(files).expect_err("one file over the ceiling must be rejected"),
        PackageSidecarError::TooManyFiles {
            count: MAX_PACKAGE_FILES + 1,
            max: MAX_PACKAGE_FILES,
        }
    );
}

#[test]
fn canonical_byte_bound_is_inclusive_at_eight_mib() {
    assert_eq!(MAX_PACKAGE_CANONICAL_BYTES, 8 * 1024 * 1024);
    let one_file_overhead = 4 + PACKAGE_SIDECAR_SCHEMA.len() + 4 + 4 + 1 + 8;
    let exact_contents = vec![0x5a; MAX_PACKAGE_CANONICAL_BYTES - one_file_overhead];
    let exact = CanonicalPackage::new(vec![PackageFile::new(b"a", exact_contents)])
        .expect("the frozen canonical-byte ceiling is inclusive");
    assert_eq!(exact.size_bytes(), MAX_PACKAGE_CANONICAL_BYTES);

    let oversized_contents = vec![0x5a; MAX_PACKAGE_CANONICAL_BYTES - one_file_overhead + 1];
    assert_eq!(
        CanonicalPackage::new(vec![PackageFile::new(b"a", oversized_contents)])
            .expect_err("one canonical byte over the ceiling must be rejected"),
        PackageSidecarError::PackageTooLarge {
            size: MAX_PACKAGE_CANONICAL_BYTES + 1,
            max: MAX_PACKAGE_CANONICAL_BYTES,
        }
    );
}

#[test]
fn package_root_uses_the_frozen_sidecar_domain_and_not_artifact_root_semantics() {
    assert_eq!(
        PACKAGE_SIDECAR_SCHEMA,
        "boole.useful-work.package-sidecar.v1"
    );
    assert_eq!(
        PACKAGE_SIDECAR_ROOT_DOMAIN,
        PACKAGE_SIDECAR_SCHEMA.as_bytes()
    );

    let package = CanonicalPackage::new(vec![PackageFile::new(b"result.bin", [1, 2, 3])])
        .expect("valid package");
    let expected = h_protocol(PACKAGE_SIDECAR_ROOT_DOMAIN, &[package.canonical_bytes()]);
    assert_eq!(package.root().as_bytes(), expected.as_bytes());
    assert_eq!(
        PackageRoot::from_hex(&package.root().to_hex()).expect("stored root round-trips"),
        package.root()
    );
    assert_eq!(
        package.root().to_hex(),
        "60cf7060649f37c2eef0ec64e52be8e65a8e7a9bf546cfac6d0b7dd3c71747eb"
    );

    let artifact_domain_root = h_protocol(
        b"boole.useful-work.artifact-root.v0",
        &[package.canonical_bytes()],
    );
    assert_ne!(package.root().as_bytes(), artifact_domain_root.as_bytes());
}

#[test]
fn canonical_package_bytes_round_trip_through_the_ingress_parser() {
    let original = CanonicalPackage::new(vec![
        PackageFile::new(b"src/lib.rs", b"pub fn answer() -> u32 { 42 }"),
        PackageFile::new(b"result.bin", [0_u8, 1, 2, 255]),
    ])
    .expect("canonical package");

    let parsed = CanonicalPackage::from_canonical_bytes(original.canonical_bytes())
        .expect("wire bytes parse");
    assert_eq!(parsed.canonical_bytes(), original.canonical_bytes());
    assert_eq!(parsed.root(), original.root());
}

#[test]
fn ingress_parser_rejects_trailing_bytes_instead_of_hashing_an_ambiguous_package() {
    let package = CanonicalPackage::new(vec![PackageFile::new(b"result.bin", [1, 2, 3])])
        .expect("canonical package");
    let mut ambiguous = package.canonical_bytes().to_vec();
    ambiguous.push(0);

    assert_eq!(
        CanonicalPackage::from_canonical_bytes(&ambiguous).unwrap_err(),
        PackageSidecarError::NonCanonicalEncoding
    );
}
