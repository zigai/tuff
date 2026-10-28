use super::*;

fn plan(
    source: &str,
    rule: SpacingRule,
    options: &SpacingOptions,
    metrics: Option<&dyn RenderedMetrics>,
) -> Vec<GapDecision> {
    let module = ruff_python_parser::parse_module(source)
        .unwrap()
        .into_syntax();
    gaps(&module, source, &[rule], options, metrics).unwrap()
}

#[test]
fn satisfied_gaps_remain_in_the_plan() {
    let source = "def f(x):\n    if x:\n        x += 1\n\n    return x\n";
    let decisions = plan(
        source,
        SpacingRule::AfterControlBlock,
        &SpacingOptions::default(),
        None,
    );
    assert_eq!(decisions.len(), 1);
    let gap = &decisions[0];
    assert_eq!(gap.blank_lines, 1);
    assert_eq!(&source[gap.range], gap.replacement);
    assert_eq!(gap.anchor.kind, GapKind::BetweenStatements);
    assert_eq!(gap.anchor.previous.map(usize::from), source.find("if x:"));
    assert_eq!(gap.anchor.next.map(usize::from), source.find("return x"));
}

#[test]
fn rendered_metrics_override_source_line_thresholds() {
    struct Metrics;
    impl RenderedMetrics for Metrics {
        fn is_own_line(&self, _: TextSize) -> Option<bool> {
            Some(true)
        }
        fn same_line(&self, _: TextSize, _: TextSize) -> Option<bool> {
            Some(false)
        }
        fn non_empty_lines(&self, _: TextRange) -> Option<usize> {
            Some(10)
        }
    }
    let source = "def f():\n    value = 1\n    return value\n";
    let mut options = SpacingOptions::default();
    options.before_branch.allow_related_return_tails = false;
    assert!(plan(source, SpacingRule::BeforeBranch, &options, None).is_empty());
    let decisions = plan(source, SpacingRule::BeforeBranch, &options, Some(&Metrics));
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].blank_lines, 1);
}

#[test]
fn settings_round_trip_and_conflicts_are_explicit() {
    let options = SpacingOptions::default();
    assert_eq!(
        options,
        toml::from_str::<SpacingOptions>(&toml::to_string(&options).unwrap()).unwrap()
    );
    assert!(
        validate(&[
            SpacingRule::BeforeUnrelatedBlock,
            SpacingRule::BlockHeaderCuddleStrict
        ])
        .is_err()
    );
    let invalid: RuleOptions = toml::from_str("related_use_lookahead = -1").unwrap();
    assert!(factory(SpacingRule::BeforeAssignment, &invalid).is_err());
    let invalid: RuleOptions = toml::from_str("bodyUsageLookahead = 4").unwrap();
    assert!(factory(SpacingRule::BeforeUnrelatedBlock, &invalid).is_err());
}

#[test]
fn trivia_ownership_distinguishes_footer_from_next_statement_separator() {
    let footer_comment = "def f(flag):\n    if flag:\n        work()\n        # belongs to if body\n    followup()\n";
    let decisions = plan(
        footer_comment,
        SpacingRule::AfterControlBlock,
        &SpacingOptions::default(),
        None,
    );
    assert_eq!(decisions.len(), 1);
    assert!(decisions[0].range.is_empty());
    let separator_comment = "def f(flag):\n    if flag:\n        work()\n    # belongs to following statement\n    followup()\n";
    assert!(
        plan(
            separator_comment,
            SpacingRule::AfterControlBlock,
            &SpacingOptions::default(),
            None
        )
        .is_empty()
    );
}

#[test]
fn source_header_is_not_a_suite_leading_line() {
    let source = "\n# module header\n\nvalue = 1\n";
    let decisions = plan(
        source,
        SpacingRule::NoSuiteLeadingTrailingBlankLines,
        &SpacingOptions::default(),
        None,
    );
    assert!(decisions.iter().all(|d| source[d.range] == d.replacement));
}

#[test]
fn capture_scopes_cover_defaults_nested_methods_and_comprehensions() {
    for definition in [
        "    def helper(value=payload):\n        return value\n",
        "    def helper():\n        return [payload for item in items]\n",
        "    class Helper:\n        payload = 1\n        def get(self):\n            return payload\n",
        "    def helper[T: payload]():\n        pass\n",
        "    class Helper[T: payload]:\n        pass\n",
    ] {
        let source = format!("def f():\n    log()\n    payload = make()\n{definition}");
        assert!(
            plan(
                &source,
                SpacingRule::BeforeAssignment,
                &SpacingOptions::default(),
                None
            )
            .is_empty(),
            "{source}"
        );
    }
    for definition in [
        "    def helper():\n        return [payload for payload in items]\n",
        "    def helper():\n        global payload\n        return payload\n",
        "    def helper():\n        def nested(payload):\n            return payload\n",
        "    def helper[payload]():\n        return payload\n",
        "    class Helper[payload]:\n        value = payload\n",
        "    def helper[payload](value=payload):\n        return value\n",
    ] {
        let source = format!("def f():\n    log()\n    payload = make()\n{definition}");
        assert_eq!(
            plan(
                &source,
                SpacingRule::BeforeAssignment,
                &SpacingOptions::default(),
                None
            )
            .len(),
            1,
            "{source}"
        );
    }
}
