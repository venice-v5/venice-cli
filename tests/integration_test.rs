use std::fs;
use venice_program_table::Vpt;

#[test]
fn test_multifile_vpt_structure() {
    // Read the generated VPT file from the multifile test
    let vpt_path = "tests/multifile-test/build/out.vpt";
    let vpt_bytes = fs::read(vpt_path).expect("Failed to read out.vpt file");

    // Decode the CBOR VPT
    let vpt = Vpt::from_bytes(&vpt_bytes).expect("Failed to decode VPT");

    // Verify entrypoint
    assert_eq!(vpt.entrypoint(), "multifile-test", "Entrypoint should be 'multifile-test'");

    // Verify we have the expected modules
    let modules = vpt.modules();
    assert_eq!(modules.len(), 3, "Should have 3 modules");

    // Check main package (__init__.py)
    let main_package = vpt.get_module("multifile-test").expect("Main package not found");
    assert!(main_package.is_package(), "multifile-test should be a package");
    assert!(!main_package.bytecode().is_empty(), "Main package should have bytecode");

    // Check utils module
    let utils = vpt.get_module("multifile-test.utils").expect("Utils module not found");
    assert!(utils.is_module(), "utils should be a regular module");
    assert!(!utils.is_package(), "utils should not be a package");
    assert!(!utils.bytecode().is_empty(), "Utils should have bytecode");

    // Check subpkg package
    let subpkg = vpt.get_module("multifile-test.subpkg").expect("Subpackage not found");
    assert!(subpkg.is_package(), "subpkg should be a package");
    assert!(!subpkg.is_module(), "subpkg should not be a regular module");
    assert!(!subpkg.bytecode().is_empty(), "Subpackage should have bytecode");

    println!("✓ VPT structure verified:");
    println!("  - Entrypoint: {}", vpt.entrypoint());
    println!("  - Modules: {}", modules.len());
    for (name, module) in modules.iter() {
        let module_type = if module.is_package() { "package" } else { "module" };
        println!("    - {} ({}, {} bytes)", name, module_type, module.bytecode().len());
    }
}
