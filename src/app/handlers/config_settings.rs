// Configuration, settings editor, key storage, and config panel dispatch
// Domain: Config hot-reload, config panel, settings editor, API key management

use crate::action::Action;
use crate::components::Component;
use crate::config::SecretString;
use crate::error::Result;
use crate::input::focus::FocusArea;

use super::super::App;

impl App {
    pub(super) fn dispatch_config_settings(&mut self, action: Action) -> Result<()> {
        match action {
            // Config actions
            Action::ConfigChanged(path) => {
                tracing::info!("Config file changed: {}", path.display());
                self.config_manager.reload_file(&path);

                // TRC-028: Handle streams.toml changes - dynamically regenerate menu from config
                if path.file_name().and_then(|n| n.to_str()) == Some("streams.toml") {
                    self.reload_streams_from_config();
                }

                // Re-apply LLM settings when llm.toml changes (fixes model not updating after hot-reload)
                if path.file_name().and_then(|n| n.to_str()) == Some("llm.toml") {
                    let llm_config = self.config_manager.llm_config();
                    self.agent.agent_engine.set_provider(&llm_config.defaults.provider);
                    self.agent.agent_engine.set_model(&llm_config.defaults.model);
                    tracing::info!(
                        "Re-applied LLM settings after hot-reload: provider={}, model={}",
                        llm_config.defaults.provider,
                        llm_config.defaults.model
                    );
                }
            }
            Action::ConfigReload => {
                tracing::info!("Reloading all configuration files");
                self.config_manager.reload_all();
            }
            Action::ConfigApplyTheme => {
                tracing::debug!("Theme changes applied");
            }

            // Config panel actions (TRC-014)
            Action::ConfigPanelShow => {
                // Refresh config panel with current settings before showing
                let providers = self.agent.agent_engine.registered_providers();
                self.config_panel.refresh(
                    self.config_manager.app_config(),
                    self.config_manager.keybindings(),
                    self.config_manager.theme(),
                    &providers,
                );
                self.show_config_panel = true;
                self.ui.focus.focus(FocusArea::ConfigPanel);
            }
            Action::ConfigPanelHide => {
                self.show_config_panel = false;
                self.ui.focus.focus(FocusArea::Menu);
            }
            Action::ConfigPanelToggle => {
                if self.show_config_panel {
                    self.show_config_panel = false;
                    self.ui.focus.focus(FocusArea::Menu);
                } else {
                    let providers = self.agent.agent_engine.registered_providers();
                    self.config_panel.refresh(
                        self.config_manager.app_config(),
                        self.config_manager.keybindings(),
                        self.config_manager.theme(),
                        &providers,
                    );
                    self.show_config_panel = true;
                    self.ui.focus.focus(FocusArea::ConfigPanel);
                }
            }
            Action::ConfigPanelScrollUp(n) => {
                self.config_panel.scroll_up(n);
            }
            Action::ConfigPanelScrollDown(n) => {
                self.config_panel.scroll_down(n);
            }
            Action::ConfigPanelScrollToTop => {
                self.config_panel.scroll_to_top();
            }
            Action::ConfigPanelScrollToBottom => {
                self.config_panel.scroll_to_bottom();
            }
            Action::ConfigPanelScrollPageUp => {
                self.config_panel.scroll_page_up();
            }
            Action::ConfigPanelScrollPageDown => {
                self.config_panel.scroll_page_down();
            }
            Action::ConfigPanelNextSection => {
                self.config_panel.next_section();
            }
            Action::ConfigPanelPrevSection => {
                self.config_panel.prev_section();
            }
            Action::ConfigPanelToggleSection => {
                self.config_panel.toggle_section();
            }

            // Settings Editor actions (TS-012)
            Action::SettingsShow => {
                self.open_settings_editor();
            }
            Action::SettingsClose => {
                self.close_settings_editor();
            }
            Action::SettingsToggle => {
                if self.show_settings_editor {
                    self.close_settings_editor();
                } else {
                    self.open_settings_editor();
                }
            }
            Action::SettingsNextSection => {
                self.settings_editor.update(&action);
            }
            Action::SettingsPrevSection => {
                self.settings_editor.update(&action);
            }
            Action::SettingsNextItem => {
                self.settings_editor.update(&action);
            }
            Action::SettingsPrevItem => {
                self.settings_editor.update(&action);
            }
            Action::SettingsScrollUp(_) => {
                self.settings_editor.update(&action);
            }
            Action::SettingsScrollDown(_) => {
                self.settings_editor.update(&action);
            }
            Action::SettingsStartEdit => {
                // Handled by settings_editor internally via handle_event
            }
            Action::SettingsCancelEdit => {
                // Handled by settings_editor internally via handle_event
            }
            Action::SettingsKeyEntered { ref provider, ref key } => {
                // Store the key in keystore and update SettingsEditor
                self.handle_settings_key_entered(provider.clone(), key.clone());
            }
            Action::SettingsProviderChanged(ref provider) => {
                // Update AgentEngine with new provider
                self.agent.agent_engine.set_provider(provider);
                // Update config_manager so it persists on save
                self.config_manager.llm_config_mut().defaults.provider = provider.clone();
                // Refresh models list for the new provider
                let models = self.agent.model_catalog.models_for_provider(provider);
                self.settings_editor.set_available_models(models.iter().map(|m| m.to_string()).collect());
            }
            Action::SettingsModelChanged(ref model) => {
                // Update AgentEngine with new model
                self.agent.agent_engine.set_model(model);
                // Update config_manager so it persists on save
                self.config_manager.llm_config_mut().defaults.model = model.clone();
            }
            Action::SettingsTestKey => {
                self.handle_settings_test_key();
            }
            Action::SettingsTestKeyResult { ref provider, success, ref error } => {
                self.settings_editor.set_key_test_result(provider, success, error.clone());
            }
            Action::SettingsTemperatureChanged(temp) => {
                // Update config with new temperature
                self.config_manager.llm_config_mut().parameters.temperature = temp;
            }
            Action::SettingsMaxTokensChanged(tokens) => {
                // Update config with new max tokens
                self.config_manager.llm_config_mut().parameters.max_tokens = tokens;
            }
            Action::SettingsSave => {
                self.handle_settings_save();
            }

            // Key storage actions (TRC-011)
            Action::KeyStore(key_id, secret) => {
                if let Some(ref mut ks) = self.keystore {
                    let secret_str = SecretString::new(secret);
                    match ks.store(&key_id, &secret_str) {
                        Ok(()) => {
                            tracing::info!("Stored API key for {}", key_id);
                            // Re-register provider with new key
                            if let Ok(Some(s)) = ks.get(&key_id) {
                                use crate::config::KeyId;
                                match key_id {
                                    KeyId::Anthropic => self.agent.agent_engine.llm_manager_mut().register_anthropic(s.expose()),
                                    KeyId::OpenAI => self.agent.agent_engine.llm_manager_mut().register_openai(s.expose()),
                                    KeyId::Gemini => self.agent.agent_engine.llm_manager_mut().register_gemini(s.expose()),
                                    KeyId::Grok => self.agent.agent_engine.llm_manager_mut().register_grok(s.expose()),
                                    KeyId::Groq => self.agent.agent_engine.llm_manager_mut().register_groq(s.expose()),
                                    KeyId::Custom(_) => {}
                                }
                            }
                        }
                        Err(e) => tracing::error!("Failed to store API key: {}", e),
                    }
                } else {
                    tracing::warn!("Keystore not initialized");
                }
            }
            Action::KeyGet(_key_id) => {
                // Key retrieval is handled internally by register_from_keystore
                // This action exists for programmatic access if needed
            }
            Action::KeyDelete(key_id) => {
                if let Some(ref mut ks) = self.keystore {
                    match ks.delete(&key_id) {
                        Ok(()) => tracing::info!("Deleted API key for {}", key_id),
                        Err(e) => tracing::error!("Failed to delete API key: {}", e),
                    }
                }
            }
            Action::KeyList => {
                if let Some(ref ks) = self.keystore {
                    match ks.list() {
                        Ok(keys) => {
                            let names: Vec<_> = keys.iter().map(|k| k.as_str()).collect();
                            tracing::info!("Stored API keys: {:?}", names);
                        }
                        Err(e) => tracing::error!("Failed to list API keys: {}", e),
                    }
                }
            }
            Action::KeyUnlock(password) => {
                if let Some(ref mut ks) = self.keystore {
                    match ks.unlock(&password) {
                        Ok(()) => {
                            tracing::info!("Keystore unlocked");
                            // Re-register providers after unlock
                            let registered = self.agent.agent_engine.llm_manager_mut().register_from_keystore(ks);
                            if !registered.is_empty() {
                                tracing::info!("Loaded API keys for providers: {:?}", registered);
                            }
                        }
                        Err(e) => tracing::error!("Failed to unlock keystore: {}", e),
                    }
                }
            }
            Action::KeyInit(password) => {
                if let Some(ref mut ks) = self.keystore {
                    match ks.init_encrypted(&password) {
                        Ok(()) => tracing::info!("Keystore initialized with encryption"),
                        Err(e) => tracing::error!("Failed to initialize keystore: {}", e),
                    }
                }
            }

            _ => unreachable!("non-config/settings action passed to dispatch_config_settings: {:?}", action),
        }
        Ok(())
    }
}
