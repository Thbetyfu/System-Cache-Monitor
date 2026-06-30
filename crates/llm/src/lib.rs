//! LLM integration via llama-cpp-4 (feature-gated behind `ai` feature).
//!
//! The model is loaded on-demand only when the user opens the "Ask AI" panel
//! and dropped when closed to free RAM. It accepts structured scan context
//! and returns natural-language suggestions — it NEVER auto-executes actions.

use anyhow::{Context, Result};
use llama_cpp_4::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};
use std::path::Path;

/// Default GGUF model path: the qwen2:1.5b blob already on disk.
pub const DEFAULT_MODEL_PATH: &str =
    r"D:\MODEL OLLAMA\blobs\sha256-405b56374e02b21122ae1469db646be0617c02928fd78e246723ebbb98dbca3e";

/// Wraps a loaded llama.cpp model and its backend.
///
/// Holds the model in memory. The context is created on-demand during generation
/// to avoid self-referential lifetimes.
pub struct LlmEngine {
    // Note: The backend must be dropped after the model.
    backend: LlamaBackend,
    model: LlamaModel,
}

impl LlmEngine {
    /// Load a GGUF model from disk. Expensive — only call on-demand.
    ///
    /// # Errors
    /// Returns an error if the backend cannot be initialized or the model cannot be loaded.
    pub fn load(model_path: &Path) -> Result<Self> {
        log::info!("Loading LLM model from: {}", model_path.display());
        let backend = LlamaBackend::init()
            .map_err(|e| anyhow::anyhow!("Failed to initialize LlamaBackend: {:?}", e))?;

        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .with_context(|| format!("Failed to load model: {}", model_path.display()))?;

        log::info!("LLM model loaded successfully.");
        Ok(Self { backend, model })
    }

    /// Generate a response given a prompt string.
    ///
    /// # Errors
    /// Returns an error if tokenization, decoding, or sampling fails.
    pub fn generate(&mut self, prompt: &str) -> Result<String> {
        log::info!("Generating response for prompt ({} chars)", prompt.len());

        // Create context on-demand for this generation run.
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(2048))
            .with_n_threads(8)
            .with_n_threads_batch(8);
        let mut ctx = self.model.new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

        // 1. Tokenize the prompt.
        let tokens = self.model.str_to_token(prompt, AddBos::Always)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {:?}", e))?;
        let n_prompt = tokens.len();
        log::info!("Prompt token count: {}", n_prompt);
        if n_prompt == 0 {
            return Ok(String::new());
        }

        // 2. Prefill all prompt tokens. We request logits ONLY for the last token
        // so that the first sample() call targets index (n_prompt - 1).
        let mut batch = LlamaBatch::new(n_prompt.max(512), 1);
        for (i, &tok) in tokens.iter().enumerate() {
            batch.add(tok, i as i32, &[0], i == n_prompt - 1)
                .map_err(|e| anyhow::anyhow!("Failed to add token to batch: {:?}", e))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| anyhow::anyhow!("Failed to decode prompt batch: {:?}", e))?;

        // 3. Setup sampler chain.
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(42),
        ]);

        // Warm up the sampler with prompt tokens so its state is primed.
        for &tok in &tokens {
            sampler.accept(tok);
        }

        let mut pos = n_prompt as i32;
        let mut raw_bytes: Vec<u8> = Vec::new();

        // 4. Generate loop (up to 512 tokens).
        // Key invariant:
        //   - First iteration: logits live at index (n_prompt - 1) in the prefill batch.
        //   - All subsequent iterations: batch has exactly 1 token, logits live at index 0.
        let mut logit_idx = (n_prompt - 1) as i32;

        for _ in 0..512 {
            let token = sampler.sample(&ctx, logit_idx);

            if self.model.is_eog_token(token) {
                break;
            }

            let bytes = self.model.token_to_bytes(token, Special::Plaintext)
                .map_err(|e| anyhow::anyhow!("Failed to convert token to bytes: {:?}", e))?;
            raw_bytes.extend_from_slice(&bytes);

            // Inform sampler of the chosen token (for repetition penalties etc.).
            sampler.accept(token);

            // Feed the generated token back. Single-token batch → logits at index 0.
            batch.clear();
            batch.add(token, pos, &[0], true)
                .map_err(|e| anyhow::anyhow!("Failed to add generated token to batch: {:?}", e))?;
            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("Failed to decode generated token batch: {:?}", e))?;
            pos += 1;

            // From this point on every batch is a single token → logits at index 0.
            logit_idx = 0;
        }

        let output = String::from_utf8_lossy(&raw_bytes).into_owned();
        log::info!("Generated response ({} chars)", output.len());
        Ok(output)
    }

    /// Check if a model file exists at the given path.
    pub fn model_available(path: &Path) -> bool {
        path.exists()
    }
}

/// Build a prompt from scan context for the LLM.
/// The prompt is deliberately structured so the LLM has clear, factual
/// input rather than guessing.
pub fn build_scan_prompt(
    results: &[ca_core::scanner::ScanResult],
    scores: &[ca_core::classifier::RiskScore],
) -> String {
    use ca_core::scanner::format_bytes;

    let mut prompt = String::from(
        "You are Cache Advisor, a storage management assistant. \
         Based on the following scan results, answer the user's question.\n\
         Be concise and practical. Focus on what is safe to clean or archive.\n\n\
         SCAN RESULTS:\n",
    );

    for (res, score) in results.iter().zip(scores.iter()) {
        if !res.stats.exists {
            continue;
        }
        prompt.push_str(&format!(
            "  - {} ({}): tier={}, urgency={}/100, files={}, stale={}/{}\n",
            res.rule.name,
            format_bytes(res.stats.total_bytes),
            match res.rule.tier {
                ca_core::rules::CleaningTier::Cache => "cache",
                ca_core::rules::CleaningTier::Cautious => "cautious",
                ca_core::rules::CleaningTier::MonitorOnly => "monitor-only",
            },
            score.urgency,
            res.stats.file_count,
            res.stats.stale_file_count,
            res.stats.file_count,
        ));
    }

    prompt.push_str(
        "\nBased on the above, which folders should the user clean or move to external storage? \
         Explain briefly for each recommendation.\n\
         Answer:",
    );
    prompt
}

/// Build a custom prompt from scan context and user's specific question.
pub fn build_custom_prompt(
    results: &[ca_core::scanner::ScanResult],
    scores: &[ca_core::classifier::RiskScore],
    question: &str,
) -> String {
    use ca_core::scanner::format_bytes;

    let mut prompt = String::from(
        "You are Cache Advisor, a storage management assistant. \
         Based on the following scan results, answer the user's question.\n\
         Be concise and practical.\n\n\
         SCAN RESULTS:\n",
    );

    for (res, score) in results.iter().zip(scores.iter()) {
        if !res.stats.exists {
            continue;
        }
        prompt.push_str(&format!(
            "  - {} ({}): tier={}, urgency={}/100, files={}, stale={}/{}\n",
            res.rule.name,
            format_bytes(res.stats.total_bytes),
            match res.rule.tier {
                ca_core::rules::CleaningTier::Cache => "cache",
                ca_core::rules::CleaningTier::Cautious => "cautious",
                ca_core::rules::CleaningTier::MonitorOnly => "monitor-only",
            },
            score.urgency,
            res.stats.file_count,
            res.stats.stale_file_count,
            res.stats.file_count,
        ));
    }

    prompt.push_str(&format!(
        "\nUser Question: {}\n\
         Answer:",
        question
    ));
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_loading_and_generation() {
        let path = Path::new(DEFAULT_MODEL_PATH);
        if path.exists() {
            println!("Testing model loading...");
            let mut engine = LlmEngine::load(path).expect("Failed to load LlmEngine");
            println!("Testing generation...");
            let prompt = "Why is the sky blue? Answer in 1 sentence.";
            let response = engine.generate(prompt).expect("Failed to generate response");
            println!("Response: {}", response);
            assert!(!response.is_empty());
        } else {
            println!("Model file not found, skipping test.");
        }
    }
}
