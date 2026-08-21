//! Protocol mappers: Anthropic / OpenAI chat / Responses → Gemini → wire formats.

pub mod anthropic;
pub mod args_fix;
pub mod history_media;
pub mod latex;
pub mod models;
pub mod openai;
pub mod responses;

pub use anthropic::{
    anthropic_to_gemini_request, gemini_to_anthropic_response, gemini_to_anthropic_sse_chunk,
};
pub use models::{list_public_models, map_model_id};
pub use openai::{
    gemini_to_openai_response, gemini_to_openai_sse_chunk, openai_to_gemini_request,
};
pub use responses::{
    gemini_to_responses_response, responses_compact_stub, responses_to_gemini_request,
    ResponsesStreamEncoder,
};
