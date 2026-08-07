//! Protocol mappers: Anthropic / OpenAI chat → Gemini request → wire formats.

pub mod anthropic;
pub mod models;
pub mod openai;

pub use anthropic::{
    anthropic_to_gemini_request, gemini_to_anthropic_response, gemini_to_anthropic_sse_chunk,
};
pub use models::{list_public_models, map_model_id};
pub use openai::{
    gemini_to_openai_response, gemini_to_openai_sse_chunk, openai_to_gemini_request,
};
