use super::estimate_cost_usd;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "actual={actual}, expected={expected}"
    );
}

#[test]
fn estimate_cost_matches_openai_gpt5_family_prices() {
    // 基准样本：输入 1000，缓存 200，输出 500
    // gpt-5 系列：输入 1.25/M，缓存 0.125/M，输出 10/M
    // => 非缓存输入 800*0.00125/1000 + 缓存 200*0.000125/1000 + 输出 500*0.01/1000
    // => 0.006025
    let expected = 0.006025_f64;
    let models = [
        "gpt-5",
        "gpt-5-codex",
        "gpt-5.1",
        "gpt-5.1-codex",
        "gpt-5.1-codex-max",
    ];
    for model in models {
        let actual = estimate_cost_usd(Some(model), Some(1000), Some(200), Some(500));
        assert_close(actual, expected);
    }
}

#[test]
fn estimate_cost_matches_openai_gpt5_mini_and_52_prices() {
    // mini：输入 0.25/M，缓存 0.025/M，输出 2/M
    // 样本同上 => 0.001205
    let mini_cost = estimate_cost_usd(Some("gpt-5.1-codex-mini"), Some(1000), Some(200), Some(500));
    assert_close(mini_cost, 0.001205);

    // 5.2：输入 1.75/M，缓存 0.175/M，输出 14/M
    // 样本同上 => 0.008435
    let v52_models = ["gpt-5.2", "gpt-5.2-codex"];
    for model in v52_models {
        let actual = estimate_cost_usd(Some(model), Some(1000), Some(200), Some(500));
        assert_close(actual, 0.008435);
    }
}

#[test]
fn estimate_cost_uses_cached_input_rate_for_gpt_5_1_codex() {
    // 非缓存输入 800k * 1.25 + 缓存输入 200k * 0.125 + 输出 500k * 10
    // 期望：1 + 0.025 + 5 = 6.025 USD
    let actual = estimate_cost_usd(
        Some("gpt-5.1-codex"),
        Some(1_000_000),
        Some(200_000),
        Some(500_000),
    );
    assert_close(actual, 6.025);
}

#[test]
fn estimate_cost_falls_back_gpt_5_3_codex_to_gpt_5_2_codex_price() {
    // gpt-5.3-codex 暂按 gpt-5.2-codex：输入 1.75 + 输出 14.00
    let actual = estimate_cost_usd(
        Some("gpt-5.3-codex"),
        Some(1_000_000),
        Some(0),
        Some(1_000_000),
    );
    assert_close(actual, 15.75);
}
