use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info, warn};

/// Configuration options for running Kani verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KaniOptions {
    /// Path to the Rust project to verify
    pub path: PathBuf,
    
    /// Specific harness to run (e.g., "module::function")
    pub harness: Option<String>,
    
    /// Run all tests as verification harnesses
    pub tests: bool,
    
    /// Output format: regular, terse, old, json
    pub output_format: String,
    
    /// Enable unstable Kani features
    pub enable_unstable: Vec<String>,
    
    /// Additional arguments to pass to Kani
    pub extra_args: Vec<String>,
    
    /// Enable concrete playback for counterexamples
    pub concrete_playback: bool,
    
    /// Enable coverage information
    pub coverage: bool,
}

impl Default for KaniOptions {
    fn default() -> Self {
        Self {
            path: PathBuf::from("."),
            harness: None,
            tests: false,
            output_format: "terse".to_string(),
            enable_unstable: vec![],
            extra_args: vec![],
            concrete_playback: false,
            coverage: false,
        }
    }
}

/// Result of a Kani verification run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether verification succeeded
    pub success: bool,
    
    /// Human-readable summary
    pub summary: String,
    
    /// List of harness results
    pub harnesses: Vec<HarnessResult>,
    
    /// Failed checks with details
    pub failed_checks: Vec<FailedCheck>,
    
    /// Verification time in seconds
    pub verification_time: Option<f64>,
    
    /// Raw output from Kani
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessResult {
    pub name: String,
    pub status: String,
    pub checks_passed: u32,
    pub checks_failed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedCheck {
    pub description: String,
    pub file: String,
    pub line: Option<u32>,
    pub function: String,
}

/// Wrapper around cargo-kani for executing verification
pub struct KaniWrapper {
    cargo_kani_path: PathBuf,
}

impl KaniWrapper {
    /// Create a new KaniWrapper, finding cargo-kani in PATH
    pub fn new() -> Result<Self> {
        let cargo_kani = which::which("cargo-kani")
            .context("cargo-kani not found in PATH. Please install Kani:\n  cargo install --locked kani-verifier\n  cargo kani setup")?;
        
        info!("✓ Found cargo-kani at: {:?}", cargo_kani);
        Ok(Self {
            cargo_kani_path: cargo_kani,
        })
    }

    /// Run Kani verification with the given options
    pub async fn verify(&self, options: KaniOptions) -> Result<VerificationResult> {
        info!("Starting Kani verification on: {:?}", options.path);
        
        if !options.path.exists() {
            anyhow::bail!("Path does not exist: {:?}", options.path);
        }

        // Build the command
        let mut cmd = Command::new(&self.cargo_kani_path);
        cmd.arg("kani");
        cmd.current_dir(&options.path);

        // Add harness filter
        if let Some(harness) = &options.harness {
            cmd.arg("--harness").arg(harness);
            info!("  Filtering to harness: {}", harness);
        }

        // Run tests as harnesses
        if options.tests {
            cmd.arg("--tests");
            info!("  Running all tests as harnesses");
        }

        // Set output format
        if !options.output_format.is_empty() {
            cmd.arg(format!("--output-format={}", options.output_format));
        }

        // Enable unstable features
        for feature in &options.enable_unstable {
            cmd.arg("--enable-unstable").arg(feature);
        }

        // Concrete playback
        if options.concrete_playback {
            cmd.arg("-Z").arg("concrete-playback");
            cmd.arg("--concrete-playback=print");
        }

        // Coverage
        if options.coverage {
            cmd.arg("--coverage");
        }

        // Extra arguments
        for arg in &options.extra_args {
            cmd.arg(arg);
        }

        debug!("Executing command: {:?}", cmd);

        // Execute and capture output
        let start = std::time::Instant::now();
        let output = cmd.output()
            .context("Failed to execute cargo-kani. Is it installed?")?;
        let duration = start.elapsed();

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let combined_output = format!("{}\n{}", stdout, stderr);

        debug!("Kani completed in {:.2}s", duration.as_secs_f64());
        
        if !stderr.is_empty() && stderr.contains("error") {
            warn!("Kani stderr: {}", stderr);
        }

        // Parse the output
        let result = self.parse_output(&combined_output, output.status.success())?;

        info!("Verification complete: {}", result.summary);
        
        Ok(result)
    }

    /// Parse Kani output into structured result
    fn parse_output(&self, output: &str, success: bool) -> Result<VerificationResult> {
        use crate::parser::KaniOutputParser;
        
        let parser = KaniOutputParser::new(output);
        let harnesses = parser.parse_harnesses();
        let failed_checks = parser.parse_failed_checks();
        let verification_time = parser.parse_verification_time();

        let total_harnesses = harnesses.len();
        let failed_harnesses = harnesses.iter()
            .filter(|h| h.status == "FAILED")
            .count();
        
        let summary = if success {
            format!("Verification successful! {} harness(es) verified.", total_harnesses)
        } else {
            format!("Verification failed. {}/{} harness(es) failed with {} check failure(s).", 
                    failed_harnesses, total_harnesses, failed_checks.len())
        };

        Ok(VerificationResult {
            success,
            summary,
            harnesses,
            failed_checks,
            verification_time,
            raw_output: output.to_string(),
        })
    }
}