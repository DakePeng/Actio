use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::engine::agent::task::{StructuredTask, TaskConfig, TaskInput};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Correction {
    pub original: String,
    pub corrected: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RefinementOutput {
    pub corrected_text: String,
    pub corrections: Vec<Correction>,
}

pub struct TextRefinementTask {
    pub hot_words: Vec<String>,
}

impl StructuredTask for TextRefinementTask {
    type Output = RefinementOutput;

    fn system_prompt(&self) -> String {
        format!(
            concat!(
                "You are a transcript correction assistant. ",
                "Fix ASR (speech recognition) errors in the text using the provided vocabulary. ",
                "Return JSON with the corrected full text and a list of corrections made.\n\n",
                "Vocabulary / hot words: {}\n\n",
                "Rules:\n",
                "- Only fix words that are likely ASR misrecognitions of the vocabulary words\n",
                "- Preserve the original meaning and structure\n",
                "- Each correction should explain what was wrong and why\n",
                "- If no corrections needed, return the original text with empty corrections array",
            ),
            self.hot_words.join(", ")
        )
    }

    fn config(&self) -> TaskConfig {
        TaskConfig {
            max_tokens: 4000,
            temperature: 0.1,
            thinking_budget: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinement_output_has_json_schema() {
        let gen = schemars::gen::SchemaSettings::draft07().into_generator();
        let schema = gen.into_root_schema_for::<RefinementOutput>();
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("corrected_text"));
        assert!(json.contains("corrections"));
    }

    #[test]
    fn system_prompt_includes_hot_words() {
        let task = TextRefinementTask {
            hot_words: vec!["Kubernetes".into(), "PostgreSQL".into()],
        };
        let prompt = task.system_prompt();
        assert!(prompt.contains("Kubernetes"));
        assert!(prompt.contains("PostgreSQL"));
    }
}
