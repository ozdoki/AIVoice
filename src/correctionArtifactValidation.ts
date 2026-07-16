import type { CorrectionArtifact, Mode } from "./types";

const MAX_ARTIFACTS = 16;
const MAX_VOCABULARY_CHARS = 128;
const MAX_REPLACEMENT_FROM_CHARS = 128;
const MAX_REPLACEMENT_TO_CHARS = 2_000;
const MAX_STYLE_EXAMPLE_CHARS = 8_000;
const OVERLY_GENERAL_REPLACEMENTS = new Set([
  "です", "ます", "する", "これ", "それ", "the", "to", "of", "in",
]);

const charCount = (value: string) => Array.from(value).length;

export function validateCorrectionArtifacts(
  artifacts: CorrectionArtifact[],
  context: { appProcess: string; mode: Mode }
): string | null {
  if (artifacts.length === 0) return "学習候補を選択してください。";
  if (artifacts.length > MAX_ARTIFACTS) {
    return `学習候補は最大${MAX_ARTIFACTS}件です。`;
  }
  const noneCount = artifacts.filter((artifact) => artifact.type === "none").length;
  if (noneCount > 0 && (noneCount !== 1 || artifacts.length !== 1)) {
    return "「適用なし」は他の学習候補と同時に選択できません。";
  }

  for (const artifact of artifacts) {
    if (artifact.type === "none") continue;
    if (artifact.type === "vocabulary") {
      if (!artifact.value.trim()) return "語彙候補が空です。値を入力するか候補を削除してください。";
      if (charCount(artifact.value) > MAX_VOCABULARY_CHARS) {
        return `語彙は最大${MAX_VOCABULARY_CHARS}文字です。`;
      }
      if (artifact.scope === "app" && !context.appProcess.trim()) {
        return "入力先アプリが不明なため、アプリ別語彙として保存できません。";
      }
      continue;
    }
    if (artifact.type === "replacement") {
      if (!artifact.from.trim() || !artifact.to.trim()) {
        return "置換候補の置換元と置換先を入力してください。";
      }
      if (artifact.from === artifact.to) return "置換元と置換先が同じです。";
      const normalizedFrom = artifact.from.trim().toLowerCase();
      if (charCount(normalizedFrom) < 2 || OVERLY_GENERAL_REPLACEMENTS.has(normalizedFrom)) {
        return "置換元が短すぎるか一般的すぎます。";
      }
      if (charCount(artifact.from) > MAX_REPLACEMENT_FROM_CHARS) {
        return `置換元は最大${MAX_REPLACEMENT_FROM_CHARS}文字です。`;
      }
      if (charCount(artifact.to) > MAX_REPLACEMENT_TO_CHARS) {
        return `置換先は最大${MAX_REPLACEMENT_TO_CHARS}文字です。`;
      }
      if (artifact.scope === "app" && !context.appProcess.trim()) {
        return "入力先アプリが不明なため、アプリ別置換として保存できません。";
      }
      continue;
    }
    if (!artifact.input.trim() || !artifact.output.trim()) {
      return "文体例の入力と出力を入力してください。";
    }
    if (artifact.input === artifact.output) return "文体例の入力と出力が同じです。";
    if (context.mode !== "polish") return "文体例はPolish履歴にだけ保存できます。";
    if (charCount(artifact.input) + charCount(artifact.output) > MAX_STYLE_EXAMPLE_CHARS) {
      return `文体例は入出力合計で最大${MAX_STYLE_EXAMPLE_CHARS}文字です。`;
    }
  }
  return null;
}
