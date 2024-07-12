use super::*;
use collections::HashMap;
use gpui::{AppContext, Global, Model, ModelContext};

#[derive(Default)]
pub struct LanguageModelRegistry {
    providers: HashMap<LanguageModelProviderName, Box<dyn LanguageModelProvider>>,
}

impl Global for LanguageModelRegistry {}

impl LanguageModelRegistry {
    pub fn register<T: LanguageModelProvider>(
        &mut self,
        provider: Model<T>,
        cx: &mut ModelContext<Self>,
    ) {
        let name = provider.name(cx);

        self.providers.insert(name, Box::new(provider));
    }

    pub fn available_models(&self, cx: &AppContext) -> Vec<AvailableLanguageModel> {
        self.providers
            .values()
            .flat_map(|provider| {
                provider
                    .provided_models(cx)
                    .into_iter()
                    .map(|model| AvailableLanguageModel {
                        provider: provider.name(cx),
                        model,
                    })
            })
            .collect()
    }

    pub fn model(
        &mut self,
        requested: &AvailableLanguageModel,
        cx: &mut AppContext,
    ) -> Result<Arc<dyn LanguageModel>> {
        let provider = self.providers.get(&requested.provider).ok_or_else(|| {
            anyhow::anyhow!("No provider found for name: {:?}", requested.provider)
        })?;

        provider.model(requested.model.id.clone(), cx)
    }
}