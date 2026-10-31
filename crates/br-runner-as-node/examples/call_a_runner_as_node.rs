use br_graph_contract::{Fields, Reason};
use br_runner_as_node::{CallerSource, JobSpec, Outcome, Payload, RunnerAsNode, SubjectSegment};
use br_runner_jobs::Content;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub question: String,
    pub answer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReviewUpdate {
    pub answer: Option<String>,
}

impl Fields for Review {
    type Update = ReviewUpdate;

    fn empty_update() -> ReviewUpdate {
        ReviewUpdate::default()
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: ReviewUpdate) -> Result<(), Reason> {
        if update.answer.is_some() {
            self.answer = update.answer;
        }
        Ok(())
    }
}

pub fn call_a_runner_as_node(
    caller_source: impl CallerSource,
) -> Result<RunnerAsNode<Review>, Box<dyn std::error::Error>> {
    let runner_type = SubjectSegment::runner_type("analyst")?;
    Ok(RunnerAsNode::<Review>::new(
        "ask_the_analyst",
        runner_type,
        |state| {
            let question = state.fields().question.clone();
            async move {
                let payload = Content {
                    message: question,
                    attachments: Vec::new(),
                };
                Ok(JobSpec::new(Payload::Kept(payload)))
            }
        },
        |_state, outcome| async move {
            match outcome {
                Outcome::Completed {
                    inline: Some(content),
                    ..
                } => Ok(ReviewUpdate {
                    answer: Some(content.message),
                }),
                Outcome::Failed { message, .. } | Outcome::Cancelled { message } => {
                    Err(message.into())
                }
                Outcome::Refusal(refusal) => Err(refusal.message.into()),
                _ => Err("the analyst gave no inline answer".into()),
            }
        },
        caller_source,
    ))
}

fn main() {
    println!("call_a_runner_as_node(caller_source) needs a caller source made by the host");
}
