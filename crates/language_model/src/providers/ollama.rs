use anyhow::{anyhow, Result};
use futures::{future::BoxFuture, stream::BoxStream, FutureExt, StreamExt};
use gpui::{AnyView, AppContext, ModelContext, Task, WeakModel};
use http::HttpClient;
use ollama::{get_models, stream_chat_completion, ChatMessage, ChatOptions, ChatRequest};
use std::{sync::Arc, time::Duration};
use ui::{prelude::*, ButtonLike, ElevationIndex};

use crate::{
    LanguageModel, LanguageModelId, LanguageModelName, LanguageModelProvider,
    LanguageModelProviderName, LanguageModelRequest, ProvidedLanguageModel, Role,
};

const OLLAMA_DOWNLOAD_URL: &str = "https://ollama.com/download";
const OLLAMA_LIBRARY_URL: &str = "https://ollama.com/library";

#[derive(Default, Debug, Clone, PartialEq)]
pub struct OllamaSettings {
    pub api_url: String,
    pub low_speed_timeout: Option<Duration>,
}

pub struct OllamaLanguageModelProvider {
    settings: OllamaSettings,
    http_client: Arc<dyn HttpClient>,
    available_models: Vec<ollama::Model>,
    handle: WeakModel<Self>,
}

impl OllamaLanguageModelProvider {
    pub fn new(http_client: Arc<dyn HttpClient>, cx: &mut ModelContext<Self>) -> Self {
        Self {
            http_client,
            settings: OllamaSettings::default(),
            available_models: Default::default(),
            handle: cx.weak_model(),
        }
    }

    fn fetch_models(&self, cx: &AppContext) -> Task<Result<()>> {
        let http_client = self.http_client.clone();
        let api_url = self.settings.api_url.clone();

        let handle = self.handle.clone();
        // As a proxy for the server being "authenticated", we'll check if its up by fetching the models
        cx.spawn(|mut cx| async move {
            let models = get_models(http_client.as_ref(), &api_url, None).await?;

            let mut models: Vec<ollama::Model> = models
                .into_iter()
                // Since there is no metadata from the Ollama API
                // indicating which models are embedding models,
                // simply filter out models with "-embed" in their name
                .filter(|model| !model.name.contains("-embed"))
                .map(|model| ollama::Model::new(&model.name))
                .collect();

            models.sort_by(|a, b| a.name.cmp(&b.name));

            handle.update(&mut cx, |this, _| {
                this.available_models = models;
            })
        })
    }
}

impl LanguageModelProvider for OllamaLanguageModelProvider {
    fn name(&self, _cx: &AppContext) -> LanguageModelProviderName {
        LanguageModelProviderName("Ollama".into())
    }

    fn provided_models(&self, _cx: &AppContext) -> Vec<ProvidedLanguageModel> {
        self.available_models
            .iter()
            .map(|model| ProvidedLanguageModel {
                id: LanguageModelId::from(model.name.clone()),
                name: LanguageModelName::from(model.name.clone()),
            })
            .collect()
    }

    fn is_authenticated(&self, _cx: &AppContext) -> bool {
        !self.available_models.is_empty()
    }

    fn authenticate(&self, cx: &AppContext) -> Task<Result<()>> {
        if self.is_authenticated(cx) {
            Task::ready(Ok(()))
        } else {
            self.fetch_models(cx)
        }
    }

    fn authentication_prompt(&self, cx: &mut WindowContext) -> AnyView {
        let handle = self.handle.clone();
        let fetch_models = Box::new(move |cx: &mut WindowContext| {
            handle
                .update(cx, |this, cx| this.fetch_models(cx))
                .unwrap_or_else(|_| Task::ready(Ok(())))
        });

        cx.new_view(|cx| DownloadOllamaMessage::new(fetch_models, cx))
            .into()
    }

    fn reset_credentials(&self, cx: &AppContext) -> Task<Result<()>> {
        self.fetch_models(cx)
    }

    fn model(&self, id: LanguageModelId, _cx: &AppContext) -> Result<Arc<dyn LanguageModel>> {
        let model = self
            .available_models
            .iter()
            .find(|model| model.name == id.0)
            .cloned()
            .ok_or_else(|| anyhow!("No model found for name: {:?}", id.0))?;

        Ok(Arc::new(OllamaLanguageModel {
            model,
            http_client: self.http_client.clone(),
            settings: self.settings.clone(),
        }))
    }
}

pub struct OllamaLanguageModel {
    model: ollama::Model,
    http_client: Arc<dyn HttpClient>,
    settings: OllamaSettings,
}

impl OllamaLanguageModel {
    fn to_ollama_request(&self, request: LanguageModelRequest) -> ChatRequest {
        ChatRequest {
            model: self.model.name.clone(),
            messages: request
                .messages
                .into_iter()
                .map(|msg| match msg.role {
                    Role::User => ChatMessage::User {
                        content: msg.content,
                    },
                    Role::Assistant => ChatMessage::Assistant {
                        content: msg.content,
                    },
                    Role::System => ChatMessage::System {
                        content: msg.content,
                    },
                })
                .collect(),
            keep_alive: self.model.keep_alive.clone().unwrap_or_default(),
            stream: true,
            options: Some(ChatOptions {
                num_ctx: Some(self.model.max_tokens),
                stop: Some(request.stop),
                temperature: Some(request.temperature),
                ..Default::default()
            }),
        }
    }
}

impl LanguageModel for OllamaLanguageModel {
    fn count_tokens(
        &self,
        request: LanguageModelRequest,
        _cx: &AppContext,
    ) -> BoxFuture<'static, Result<usize>> {
        // There is no endpoint for this _yet_ in Ollama
        // see: https://github.com/ollama/ollama/issues/1716 and https://github.com/ollama/ollama/issues/3582
        let token_count = request
            .messages
            .iter()
            .map(|msg| msg.content.chars().count())
            .sum::<usize>()
            / 4;

        async move { Ok(token_count) }.boxed()
    }

    fn complete(
        &self,
        request: LanguageModelRequest,
        _cx: &mut AppContext,
    ) -> BoxFuture<'static, Result<BoxStream<'static, Result<String>>>> {
        let request = self.to_ollama_request(request);

        let http_client = self.http_client.clone();
        let api_url = self.settings.api_url.clone();
        let low_speed_timeout = self.settings.low_speed_timeout;
        async move {
            let request =
                stream_chat_completion(http_client.as_ref(), &api_url, request, low_speed_timeout);
            let response = request.await?;
            let stream = response
                .filter_map(|response| async move {
                    match response {
                        Ok(delta) => {
                            let content = match delta.message {
                                ChatMessage::User { content } => content,
                                ChatMessage::Assistant { content } => content,
                                ChatMessage::System { content } => content,
                            };
                            Some(Ok(content))
                        }
                        Err(error) => Some(Err(error)),
                    }
                })
                .boxed();
            Ok(stream)
        }
        .boxed()
    }
}

struct DownloadOllamaMessage {
    retry_connection: Box<dyn Fn(&mut WindowContext) -> Task<Result<()>>>,
}

impl DownloadOllamaMessage {
    pub fn new(
        retry_connection: Box<dyn Fn(&mut WindowContext) -> Task<Result<()>>>,
        _cx: &mut ViewContext<Self>,
    ) -> Self {
        Self { retry_connection }
    }

    fn render_download_button(&self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        ButtonLike::new("download_ollama_button")
            .style(ButtonStyle::Filled)
            .size(ButtonSize::Large)
            .layer(ElevationIndex::ModalSurface)
            .child(Label::new("Get Ollama"))
            .on_click(move |_, cx| cx.open_url(OLLAMA_DOWNLOAD_URL))
    }

    fn render_retry_button(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        ButtonLike::new("retry_ollama_models")
            .style(ButtonStyle::Filled)
            .size(ButtonSize::Large)
            .layer(ElevationIndex::ModalSurface)
            .child(Label::new("Retry"))
            .on_click(cx.listener(move |this, _, cx| {
                let connected = (this.retry_connection)(cx);

                cx.spawn(|_this, _cx| async move {
                    connected.await?;
                    anyhow::Ok(())
                })
                .detach_and_log_err(cx)
            }))
    }

    fn render_next_steps(&self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        v_flex()
            .p_4()
            .size_full()
            .gap_2()
            .child(
                Label::new("Once Ollama is on your machine, make sure to download a model or two.")
                    .size(LabelSize::Large),
            )
            .child(
                h_flex().w_full().p_4().justify_center().gap_2().child(
                    ButtonLike::new("view-models")
                        .style(ButtonStyle::Filled)
                        .size(ButtonSize::Large)
                        .layer(ElevationIndex::ModalSurface)
                        .child(Label::new("View Available Models"))
                        .on_click(move |_, cx| cx.open_url(OLLAMA_LIBRARY_URL)),
                ),
            )
    }
}

impl Render for DownloadOllamaMessage {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        v_flex()
            .p_4()
            .size_full()
            .gap_2()
            .child(Label::new("To use Ollama models via the assistant, Ollama must be running on your machine with at least one model downloaded.").size(LabelSize::Large))
            .child(
                h_flex()
                    .w_full()
                    .p_4()
                    .justify_center()
                    .gap_2()
                    .child(
                        self.render_download_button(cx)
                    )
                    .child(
                        self.render_retry_button(cx)
                    )
            )
            .child(self.render_next_steps(cx))
            .into_any()
    }
}
