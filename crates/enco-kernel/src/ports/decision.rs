use async_trait::async_trait;
use enco_core::{Failure, ProviderSettings};

/// Typed judgments consumed by host policies, independently of Session completion.
#[async_trait]
pub trait Decision: Send + Sync {
    /// Evaluate each question against the same state; answers follow question order.
    async fn decide(
        &self,
        settings: &ProviderSettings,
        api_key: Option<&str>,
        state: String,
        questions: Vec<Question>,
    ) -> Result<Vec<Answer>, Failure>;
}

/// Instructions and the allowed answer shape for one independent judgment.
#[derive(Clone, Debug)]
pub struct Question {
    /// The judgment to make against the shared state.
    pub instructions: String,
    /// Allowed outcomes and their interpretation.
    pub kind: QuestionKind,
}

/// A predicate, unordered alternatives, or score levels ordered from low to high.
#[derive(Clone, Debug)]
pub enum QuestionKind {
    /// A yes/no judgment.
    Predicate,
    /// Mutually exclusive alternatives.
    Choice(Vec<Label>),
    /// Ordered levels, from low to high.
    Score(Vec<Label>),
}

/// An option or score level. Names are unique within a question.
#[derive(Clone, Debug)]
pub struct Label {
    /// Option name or score-level name.
    pub name: String,
    /// Optional rubric explaining when the label applies.
    pub description: Option<String>,
}

/// Finite probabilities in [0, 1], or a refusal for this question.
/// Choice and score distributions follow label order and sum to one, allowing rounding.
#[derive(Debug)]
pub enum Answer {
    /// Probability that the predicate is true.
    Predicate(f64),
    /// Probability of each alternative, in label order.
    Choice(Vec<f64>),
    /// Probability of each level, in label order.
    Score(Vec<f64>),
    /// The service declined to answer this question.
    Refused,
}
