use crate::{StreamEvent, StreamMetadata};
use std::{fmt::Debug, marker::PhantomData};

#[derive(Clone, Debug, PartialEq, Eq)]
enum ActiveBlockKind {
    None,
    Text,
    Reasoning { id: String },
}

pub struct BlockTracker<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    active: ActiveBlockKind,
    reasoning_seq: usize,
    state: PhantomData<S>,
}

impl<S> BlockTracker<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    pub fn new() -> Self {
        Self {
            active: ActiveBlockKind::None,
            reasoning_seq: 0,
            state: PhantomData,
        }
    }

    pub fn on_text_delta(&mut self, text: &str, metadata: &StreamMetadata) -> Vec<StreamEvent<S>> {
        let mut events = Vec::new();
        if self.active != ActiveBlockKind::Text {
            events.extend(self.close_current(metadata));
            self.active = ActiveBlockKind::Text;
            events.push(StreamEvent::TextBlockStart {
                metadata: metadata.clone(),
            });
        }
        events.push(StreamEvent::TextDelta {
            content: text.to_string(),
            metadata: metadata.clone(),
        });
        events
    }

    pub fn on_reasoning_delta(
        &mut self,
        text: &str,
        metadata: &StreamMetadata,
    ) -> Vec<StreamEvent<S>> {
        let mut events = Vec::new();
        let id = match &self.active {
            ActiveBlockKind::Reasoning { id } => id.clone(),
            _ => {
                events.extend(self.close_current(metadata));
                let id = format!("r{}", self.reasoning_seq);
                self.reasoning_seq += 1;
                self.active = ActiveBlockKind::Reasoning { id: id.clone() };
                events.push(StreamEvent::ReasoningBlockStart {
                    id: id.clone(),
                    metadata: metadata.clone(),
                });
                id
            }
        };
        events.push(StreamEvent::ReasoningDelta {
            id,
            content: text.to_string(),
            metadata: metadata.clone(),
        });
        events
    }

    pub fn on_finish(&mut self, metadata: &StreamMetadata) -> Vec<StreamEvent<S>> {
        let mut events = self.close_current(metadata);
        events.push(StreamEvent::Finish);
        events
    }

    pub fn close_current(&mut self, metadata: &StreamMetadata) -> Vec<StreamEvent<S>> {
        match std::mem::replace(&mut self.active, ActiveBlockKind::None) {
            ActiveBlockKind::None => Vec::new(),
            ActiveBlockKind::Text => vec![StreamEvent::TextBlockEnd {
                metadata: metadata.clone(),
            }],
            ActiveBlockKind::Reasoning { id } => vec![StreamEvent::ReasoningBlockEnd {
                id,
                metadata: metadata.clone(),
            }],
        }
    }
}

impl<S> Default for BlockTracker<S>
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}
