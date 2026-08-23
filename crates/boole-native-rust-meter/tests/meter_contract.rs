use boole_native_rust_meter::{MeterError, MeterLimits, TupleField, TupleItem};

fn limits() -> MeterLimits {
    MeterLimits {
        max_source_bytes: 8_192,
        max_tokens: 512,
        max_ast_nodes: 256,
        max_ast_depth: 32,
        max_operations: 1_000_000,
        max_fuel: 2_000_000,
        max_prefix_items: 2_080,
    }
}

fn canonical_source() -> &'static str {
    r#"
        let mut acc: i64 = 7;
        for it in items {
            let flag: i64 = if it.1 { 1 } else { 0 };
            let v = (it.0 as i64)
                .wrapping_mul(3)
                .wrapping_add(flag.wrapping_mul(-2));
            acc = acc.wrapping_mul(5).wrapping_add(v);
        }
        acc
    "#
}

#[test]
fn literal_program_is_parsed_and_evaluated_through_the_public_meter() {
    let program = boole_native_rust_meter::parse("7", limits()).unwrap();
    let items = vec![TupleItem::new(vec![TupleField::Unsigned(11)])];
    let evaluation = program.evaluate_hidden_prefixes(&items, limits()).unwrap();

    assert_eq!(evaluation.outputs(), &[7]);
    assert_eq!(evaluation.resource_use().source_bytes(), 1);
    assert_eq!(evaluation.resource_use().prefix_items(), 0);
}

#[test]
fn tuple_projection_program_matches_the_family_hidden_prefix_semantics() {
    let program = boole_native_rust_meter::parse(canonical_source(), limits()).unwrap();
    let items = vec![
        TupleItem::new(vec![TupleField::Unsigned(2), TupleField::Bool(true)]),
        TupleItem::new(vec![TupleField::Unsigned(3), TupleField::Bool(false)]),
    ];
    let evaluation = program.evaluate_hidden_prefixes(&items, limits()).unwrap();

    assert_eq!(evaluation.outputs(), &[39, 204]);
    assert_eq!(evaluation.resource_use().prefix_items(), 3);
    assert!(evaluation.resource_use().tokens() > 20);
    assert!(evaluation.resource_use().ast_nodes() > 10);
    assert!(evaluation.resource_use().max_ast_depth() >= 4);
    assert!(evaluation.resource_use().operations() > 0);
    assert!(evaluation.resource_use().fuel() > 0);
}

#[test]
fn every_static_and_dynamic_counter_accepts_n_and_rejects_n_plus_one() {
    let baseline = boole_native_rust_meter::parse(canonical_source(), limits()).unwrap();
    let static_use = baseline.static_resource_use();

    let static_bounds = [
        ("source_bytes", static_use.source_bytes()),
        ("tokens", static_use.tokens()),
        ("ast_nodes", static_use.ast_nodes()),
        ("ast_depth", static_use.max_ast_depth()),
    ];
    for (name, exact) in static_bounds {
        let mut exact_limits = limits();
        match name {
            "source_bytes" => exact_limits.max_source_bytes = exact,
            "tokens" => exact_limits.max_tokens = exact,
            "ast_nodes" => exact_limits.max_ast_nodes = exact,
            "ast_depth" => exact_limits.max_ast_depth = exact,
            _ => unreachable!(),
        }
        boole_native_rust_meter::parse(canonical_source(), exact_limits).unwrap();

        let mut one_too_small = exact_limits;
        match name {
            "source_bytes" => one_too_small.max_source_bytes = exact - 1,
            "tokens" => one_too_small.max_tokens = exact - 1,
            "ast_nodes" => one_too_small.max_ast_nodes = exact - 1,
            "ast_depth" => one_too_small.max_ast_depth = exact - 1,
            _ => unreachable!(),
        }
        assert_eq!(
            boole_native_rust_meter::parse(canonical_source(), one_too_small).unwrap_err(),
            MeterError::BudgetExceeded(name)
        );
    }

    let items = (0..64)
        .map(|value| {
            TupleItem::new(vec![
                TupleField::Unsigned(value),
                TupleField::Bool(value % 2 == 0),
            ])
        })
        .collect::<Vec<_>>();
    let baseline_evaluation = baseline.evaluate_hidden_prefixes(&items, limits()).unwrap();
    let dynamic_use = baseline_evaluation.resource_use();
    assert_eq!(dynamic_use.prefix_items(), 2_080);
    assert_eq!(dynamic_use.operations(), 25_024);
    assert_eq!(dynamic_use.fuel(), 46_080);

    for (name, exact) in [
        ("operations", dynamic_use.operations()),
        ("fuel", dynamic_use.fuel()),
        ("prefix_items", 2_080),
    ] {
        let mut exact_limits = limits();
        match name {
            "operations" => exact_limits.max_operations = exact,
            "fuel" => exact_limits.max_fuel = exact,
            "prefix_items" => exact_limits.max_prefix_items = exact,
            _ => unreachable!(),
        }
        baseline
            .evaluate_hidden_prefixes(&items, exact_limits)
            .unwrap();

        let mut one_too_small = exact_limits;
        match name {
            "operations" => one_too_small.max_operations = exact - 1,
            "fuel" => one_too_small.max_fuel = exact - 1,
            "prefix_items" => one_too_small.max_prefix_items = exact - 1,
            _ => unreachable!(),
        }
        assert_eq!(
            baseline
                .evaluate_hidden_prefixes(&items, one_too_small)
                .unwrap_err(),
            MeterError::BudgetExceeded(name)
        );
    }
}

#[test]
fn canonical_resource_bytes_are_host_independent_and_overflow_fails_closed() {
    let program = boole_native_rust_meter::parse(canonical_source(), limits()).unwrap();
    let items = vec![TupleItem::new(vec![
        TupleField::Unsigned(5),
        TupleField::Bool(true),
    ])];
    let first = program
        .evaluate_hidden_prefixes(&items, limits())
        .unwrap()
        .resource_use();

    // Unrelated host activity cannot enter the pure meter result.
    let unrelated_host_allocation = vec![0xabu8; 65_537];
    assert_eq!(unrelated_host_allocation.len(), 65_537);
    let second = program
        .evaluate_hidden_prefixes(&items, limits())
        .unwrap()
        .resource_use();
    assert_eq!(first, second);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(
        boole_native_rust_meter::ResourceUse::from_canonical_bytes(&first.canonical_bytes())
            .unwrap(),
        first
    );

    let mut maximum = first.canonical_bytes();
    let counter_start = maximum.len() - 7 * 8;
    maximum[counter_start..counter_start + 8].copy_from_slice(&u64::MAX.to_be_bytes());
    let maximum = boole_native_rust_meter::ResourceUse::from_canonical_bytes(&maximum).unwrap();
    assert_eq!(maximum.try_add(first), Err(MeterError::CounterOverflow));
}

#[test]
fn prefix_work_is_preflighted_with_checked_arithmetic_before_evaluation() {
    let program = boole_native_rust_meter::parse(canonical_source(), limits()).unwrap();
    assert_eq!(program.required_prefix_items(64).unwrap(), 2_080);
    assert_eq!(
        program.required_prefix_items(u64::MAX),
        Err(MeterError::CounterOverflow)
    );

    let constant = boole_native_rust_meter::parse("7", limits()).unwrap();
    assert_eq!(constant.required_prefix_items(u64::MAX).unwrap(), 0);
}

#[test]
fn caller_depth_headroom_does_not_disable_the_absolute_parser_safety_cap() {
    let mut roomy = limits();
    roomy.max_ast_depth = 1_000;
    roomy.max_tokens = 1_000;
    boole_native_rust_meter::parse("7", roomy).unwrap();

    let nested = format!("{}7{}", "(".repeat(300), ")".repeat(300));
    assert_eq!(
        boole_native_rust_meter::parse(&nested, roomy).unwrap_err(),
        MeterError::BudgetExceeded("ast_depth")
    );
}

#[test]
fn tuple_field_indices_are_architecture_independent_and_family_bounded() {
    boole_native_rust_meter::parse("for it in items { let x = it.7; } 0", limits()).unwrap();
    assert_eq!(
        boole_native_rust_meter::parse("for it in items { let x = it.8; } 0", limits())
            .unwrap_err(),
        MeterError::InvalidSyntax
    );
    assert_eq!(
        boole_native_rust_meter::parse("for it in items { let x = it.4294967296; } 0", limits())
            .unwrap_err(),
        MeterError::InvalidSyntax
    );
}

#[test]
fn pure_evaluator_enforces_declared_and_inferred_binding_types() {
    let declared = boole_native_rust_meter::parse("let x: i64 = true; 0", limits()).unwrap();
    assert_eq!(
        declared
            .evaluate_hidden_prefixes(&[TupleItem::new(vec![])], limits())
            .unwrap_err(),
        MeterError::InvalidProgram("declared_i64_type_mismatch")
    );

    let inferred = boole_native_rust_meter::parse("let mut x = 0; x = true; 0", limits()).unwrap();
    assert_eq!(
        inferred
            .evaluate_hidden_prefixes(&[TupleItem::new(vec![])], limits())
            .unwrap_err(),
        MeterError::InvalidProgram("assignment_type_mismatch")
    );
}

#[test]
fn pure_evaluator_rejects_unreachable_type_errors_and_mixed_tuple_schemas() {
    let unreachable =
        boole_native_rust_meter::parse("if true { 1 } else { false }", limits()).unwrap();
    assert_eq!(
        unreachable
            .evaluate_hidden_prefixes(&[TupleItem::new(vec![])], limits())
            .unwrap_err(),
        MeterError::InvalidProgram("if_branch_type_mismatch")
    );

    let projection = boole_native_rust_meter::parse(canonical_source(), limits()).unwrap();
    let mixed = vec![
        TupleItem::new(vec![TupleField::Unsigned(1), TupleField::Bool(true)]),
        TupleItem::new(vec![TupleField::Unsigned(1), TupleField::Unsigned(1)]),
    ];
    assert_eq!(
        projection
            .evaluate_hidden_prefixes(&mixed, limits())
            .unwrap_err(),
        MeterError::InvalidProgram("tuple_schema_mismatch")
    );
}

#[test]
fn numeric_tuple_fields_require_an_explicit_i64_cast_before_arithmetic() {
    let source = "for it in items { let x: i64 = it.0; } 0";
    let program = boole_native_rust_meter::parse(source, limits()).unwrap();
    assert_eq!(
        program
            .evaluate_hidden_prefixes(&[TupleItem::new(vec![TupleField::Unsigned(1)])], limits())
            .unwrap_err(),
        MeterError::InvalidProgram("declared_i64_type_mismatch")
    );
}

#[test]
fn comments_and_all_string_forms_are_rejected_before_ast_construction() {
    for source in [
        "7 // while true {}",
        "7 /* loop {} */",
        r#""loop {}""#,
        r##"r#"loop {}"#"##,
        r#"b"loop {}""#,
        r##"br#"loop {}"#"##,
        "'x'",
    ] {
        let expected = if source.contains("//") || source.contains("/*") {
            MeterError::ForbiddenConstruct("comment")
        } else {
            MeterError::ForbiddenConstruct("string_or_char")
        };
        assert_eq!(
            boole_native_rust_meter::parse(source, limits()).unwrap_err(),
            expected
        );
    }
}

#[test]
fn compiler_escape_hatches_are_rejected_by_the_meter_parser() {
    let cases = [
        ("loop {} 0", MeterError::ForbiddenConstruct("loop")),
        ("while true {} 0", MeterError::ForbiddenConstruct("loop")),
        (
            "for a in items { for b in items {} } 0",
            MeterError::ForbiddenConstruct("nested_loop"),
        ),
        (
            "for a in items {} for b in items {} 0",
            MeterError::ForbiddenConstruct("multiple_loop"),
        ),
        (
            "acfr_solve(items)",
            MeterError::ForbiddenConstruct("recursion"),
        ),
        (
            "fn helper() {} 0",
            MeterError::ForbiddenConstruct("local_item"),
        ),
        (
            "let x: Vec<i64> = 0; x",
            MeterError::ForbiddenConstruct("generic"),
        ),
        (
            "const X: i64 = 0; X",
            MeterError::ForbiddenConstruct("const"),
        ),
        ("panic!()", MeterError::ForbiddenConstruct("macro")),
        (
            "let f = |x| x; 0",
            MeterError::ForbiddenConstruct("closure"),
        ),
        (
            "helper(1)",
            MeterError::ForbiddenConstruct("arbitrary_call"),
        ),
        ("7;", MeterError::ForbiddenConstruct("trailing_syntax")),
    ];

    for (source, expected) in cases {
        assert_eq!(
            boole_native_rust_meter::parse(source, limits()).unwrap_err(),
            expected
        );
    }
}
