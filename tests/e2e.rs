mod common;

#[test]
fn e2e_table_driven_fixtures() {
    // Each scenario: (fixture path, expected_date_substrings)
    let scenarios = vec![
        ("tests/sample.data", vec!["2026-01-05"]),
        ("tests/sample2.data", vec!["2026-01-05", "2026-02-05"]),
        ("tests/fixtures/small.data", vec!["2026-01-05"]),
        ("tests/fixtures/corrupt.data", vec!["2026-01-05"]),
        ("tests/fixtures/literal.data", vec!["2026-01-05"]),
    ];

    for (fixture, expected_dates) in scenarios {
        let (_tmp, entries) = common::replay_fixture_and_collect(fixture);
        // There should be at least one file created
        assert!(!entries.is_empty(), "no files created for fixture {}", fixture);
        // Check that each expected date substring appears in at least one filename
        for date in expected_dates {
            assert!(entries.iter().any(|name| name.contains(date)),
                "expected date {} not found in {}: {:?}", date, fixture, entries);
        }
    }
}
