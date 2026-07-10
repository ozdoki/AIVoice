use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use aivoice::{polish, settings::AppSettings};
use serde::{Deserialize, Serialize};

const CORPUS_JSON: &str = include_str!("../eval/polish-preset-corpus.json");
const KEYRING_SERVICE: &str = "aivoice";
const KEYRING_USER: &str = "api_key";
const PRESETS: [&str; 5] = ["slack", "email", "memo", "prompt", "technical"];

#[derive(Debug, Deserialize)]
struct Corpus {
    version: u32,
    cases: Vec<EvalCase>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EvalCase {
    id: String,
    preset: String,
    input: String,
    required_terms: Vec<String>,
    forbidden_terms: Vec<String>,
    required_headings: Vec<String>,
    #[serde(default)]
    required_labels: Vec<String>,
    min_bullets: usize,
    min_blank_lines: usize,
    notes: String,
}

#[derive(Debug, Serialize)]
struct CheckResult {
    passed: bool,
    missing_required_terms: Vec<String>,
    present_forbidden_terms: Vec<String>,
    missing_headings: Vec<String>,
    missing_labels: Vec<String>,
    bullet_count: usize,
    blank_line_count: usize,
}

#[derive(Debug, Serialize)]
struct HumanScores {
    meaning_fidelity: Option<u8>,
    no_invention: Option<u8>,
    cleanup_quality: Option<u8>,
    preset_fit: Option<u8>,
    readability: Option<u8>,
    token_and_structure_preservation: Option<u8>,
    notes: String,
}

#[derive(Debug, Serialize)]
struct EvalResult {
    case: EvalCase,
    output: Option<String>,
    error: Option<String>,
    machine: Option<CheckResult>,
    human: HumanScores,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u32,
    corpus_version: u32,
    generated_at_unix_seconds: u64,
    model: String,
    api_base_url: String,
    dry_run: bool,
    results: Vec<EvalResult>,
}

#[derive(Debug)]
struct Args {
    model: String,
    api_base_url: String,
    output: PathBuf,
    presets: Vec<String>,
    cases: Vec<String>,
    dry_run: bool,
    list: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            model: "gpt-5.4-mini".to_string(),
            api_base_url: "https://api.openai.com/v1".to_string(),
            output: PathBuf::from("polish-preset-eval-results.json"),
            presets: Vec::new(),
            cases: Vec::new(),
            dry_run: false,
            list: false,
        }
    }
}

fn usage() -> &'static str {
    "Usage: cargo run --example polish_preset_eval -- [--dry-run] [--list] \
     [--model MODEL] [--api-base-url URL] [--preset PRESET]... [--case ID]... \
     [--output PATH]\n\
     API execution reads the KoeType key from Windows Credential Manager and never serializes it."
}

fn parse_args() -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => parsed.model = args.next().ok_or("--model requires a value")?,
            "--api-base-url" => {
                parsed.api_base_url = args.next().ok_or("--api-base-url requires a value")?
            }
            "--output" => {
                parsed.output = PathBuf::from(args.next().ok_or("--output requires a value")?)
            }
            "--preset" => parsed
                .presets
                .push(args.next().ok_or("--preset requires a value")?),
            "--case" => parsed
                .cases
                .push(args.next().ok_or("--case requires a value")?),
            "--dry-run" => parsed.dry_run = true,
            "--list" => parsed.list = true,
            "--help" | "-h" => return Err(usage().to_string()),
            _ => return Err(format!("unknown argument: {arg}\n{}", usage())),
        }
    }
    for preset in &parsed.presets {
        if !PRESETS.contains(&preset.as_str()) {
            return Err(format!("unknown preset: {preset}"));
        }
    }
    Ok(parsed)
}

fn load_corpus() -> Result<Corpus, String> {
    let corpus: Corpus = serde_json::from_str(CORPUS_JSON).map_err(|error| error.to_string())?;
    let mut counts = BTreeMap::new();
    for case in &corpus.cases {
        if !PRESETS.contains(&case.preset.as_str()) {
            return Err(format!(
                "case {} has unknown preset {}",
                case.id, case.preset
            ));
        }
        *counts.entry(case.preset.as_str()).or_insert(0_usize) += 1;
    }
    for preset in PRESETS {
        if counts.get(preset).copied().unwrap_or_default() < 2 {
            return Err(format!("preset {preset} must have at least two cases"));
        }
    }
    Ok(corpus)
}

fn is_bullet(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ")
}

fn check_output(case: &EvalCase, output: &str) -> CheckResult {
    let missing_required_terms = case
        .required_terms
        .iter()
        .filter(|term| !output.contains(term.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let present_forbidden_terms = case
        .forbidden_terms
        .iter()
        .filter(|term| output.contains(term.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let lines = output.lines().collect::<Vec<_>>();
    let missing_headings = case
        .required_headings
        .iter()
        .filter(|heading| !lines.iter().any(|line| line.trim() == heading.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let missing_labels = case
        .required_labels
        .iter()
        .filter(|label| {
            let expected = format!("{label}:");
            !lines.iter().any(|line| {
                let line = line.trim_start();
                let line = line
                    .strip_prefix("- ")
                    .or_else(|| line.strip_prefix("* "))
                    .or_else(|| line.strip_prefix("• "))
                    .unwrap_or(line);
                line.starts_with(&expected)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let bullet_count = lines.iter().filter(|line| is_bullet(line)).count();
    let blank_line_count = lines.iter().filter(|line| line.trim().is_empty()).count();
    let passed = missing_required_terms.is_empty()
        && present_forbidden_terms.is_empty()
        && missing_headings.is_empty()
        && missing_labels.is_empty()
        && bullet_count >= case.min_bullets
        && blank_line_count >= case.min_blank_lines;
    CheckResult {
        passed,
        missing_required_terms,
        present_forbidden_terms,
        missing_headings,
        missing_labels,
        bullet_count,
        blank_line_count,
    }
}

fn empty_human_scores() -> HumanScores {
    HumanScores {
        meaning_fidelity: None,
        no_invention: None,
        cleanup_quality: None,
        preset_fit: None,
        readability: None,
        token_and_structure_preservation: None,
        notes: String::new(),
    }
}

fn selected_cases(corpus: &Corpus, args: &Args) -> Result<Vec<EvalCase>, String> {
    let selected = corpus
        .cases
        .iter()
        .filter(|case| args.presets.is_empty() || args.presets.contains(&case.preset))
        .filter(|case| args.cases.is_empty() || args.cases.contains(&case.id))
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err("no cases matched the supplied filters".to_string());
    }
    Ok(selected)
}

fn load_api_key() -> Result<String, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|error| format!("could not access Windows Credential Manager: {error}"))?;
    let key = entry
        .get_password()
        .map_err(|_| "no KoeType API key was found in Windows Credential Manager".to_string())?;
    if key.trim().is_empty() {
        return Err("the saved KoeType API key is empty".to_string());
    }
    Ok(key)
}

fn write_report(path: &Path, report: &Report) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(report).map_err(|error| error.to_string())?;
    fs::write(path, json).map_err(|error| error.to_string())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    let corpus = load_corpus()?;
    if args.list {
        for case in &corpus.cases {
            println!("{}\t{}", case.preset, case.id);
        }
        return Ok(());
    }
    let cases = selected_cases(&corpus, &args)?;
    let api_key = if args.dry_run {
        String::new()
    } else {
        load_api_key()?
    };
    let mut results = Vec::with_capacity(cases.len());
    for case in cases {
        if args.dry_run {
            results.push(EvalResult {
                case,
                output: None,
                error: None,
                machine: None,
                human: empty_human_scores(),
            });
            continue;
        }
        let settings = AppSettings {
            api_base_url: args.api_base_url.clone(),
            api_key: api_key.clone(),
            polish_model: args.model.clone(),
            polish_preset: case.preset.clone(),
            custom_polish_instructions: String::new(),
            deep_context_enabled: false,
            ..AppSettings::default()
        };
        match polish::polish_text(&settings, &[], None, &case.input).await {
            Ok(output) => {
                let machine = check_output(&case, &output);
                results.push(EvalResult {
                    case,
                    output: Some(output),
                    error: None,
                    machine: Some(machine),
                    human: empty_human_scores(),
                });
            }
            Err(error) => results.push(EvalResult {
                case,
                output: None,
                error: Some(error.to_string()),
                machine: None,
                human: empty_human_scores(),
            }),
        }
    }
    let report = Report {
        schema_version: 1,
        corpus_version: corpus.version,
        generated_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs(),
        model: args.model,
        api_base_url: args.api_base_url,
        dry_run: args.dry_run,
        results,
    };
    write_report(&args.output, &report)?;
    let evaluated = report.results.len();
    let machine_passed = report
        .results
        .iter()
        .filter(|result| result.machine.as_ref().is_some_and(|check| check.passed))
        .count();
    let errors = report
        .results
        .iter()
        .filter(|result| result.error.is_some())
        .count();
    println!(
        "Wrote {} cases to {} (machine pass: {}, errors: {}, dry-run: {})",
        evaluated,
        args.output.display(),
        machine_passed,
        errors,
        args.dry_run
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_has_two_cases_for_every_preset() {
        let corpus = load_corpus().unwrap();
        for preset in PRESETS {
            assert!(
                corpus
                    .cases
                    .iter()
                    .filter(|case| case.preset == preset)
                    .count()
                    >= 2
            );
        }
    }

    #[test]
    fn machine_check_detects_missing_forbidden_and_structure_failures() {
        let case = EvalCase {
            id: "test".into(),
            preset: "prompt".into(),
            input: "input".into(),
            required_terms: vec!["GitHub".into()],
            forbidden_terms: vec!["完了しました".into()],
            required_headings: vec!["目的".into(), "タスク".into()],
            required_labels: vec!["Check".into()],
            min_bullets: 2,
            min_blank_lines: 1,
            notes: String::new(),
        };
        let result = check_output(&case, "目的\n\n完了しました\n- 1件");
        assert!(!result.passed);
        assert_eq!(result.missing_required_terms, ["GitHub"]);
        assert_eq!(result.present_forbidden_terms, ["完了しました"]);
        assert_eq!(result.missing_headings, ["タスク"]);
        assert_eq!(result.missing_labels, ["Check"]);
        assert_eq!(result.bullet_count, 1);
    }
}
