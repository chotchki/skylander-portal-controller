//! Credits overlay (PLAN 3.19.6).
//!
//! Full-screen scrim + framed panel surfacing the attributions required
//! by the data + fonts + reverse-engineering references this app
//! bundles. Triggered from the profile-picker footer's "CREDITS"
//! button. CC BY-SA 3.0 requires discoverable attribution; "CREDITS" in
//! the footer + modal body satisfies the license per the
//! reasonable-manner clause.

use leptos::prelude::*;

#[component]
pub fn CreditsOverlay(open: RwSignal<bool>) -> impl IntoView {
    let close = move |_| open.set(false);

    view! {
        <Show when=move || open.get() fallback=|| ()>
            <div class="credits-overlay" role="dialog" aria-modal="true" aria-label="Credits">
                <div class="credits-scrim" on:click=close></div>
                <div class="credits-card">
                    <h2 class="credits-title">"CREDITS"</h2>
                    <div class="credits-body">
                        <section class="credits-disclaimer">
                            <p>
                                "Skylanders, its characters, names, logos, and artwork are \
                                 trademarks and copyright of "
                                <strong>"Activision Publishing, Inc."</strong>
                                " This app is an unofficial fan tool, built by a parent for \
                                 his kids. It is "
                                <strong>"not affiliated with, endorsed by, or sponsored by"</strong>
                                " Activision Publishing, Inc. or the Skylanders Fandom Wiki."
                            </p>
                            <p>
                                "Figure images are used in low resolution for identification \
                                 only. Users must own the corresponding physical figures and \
                                 firmware backups; no game assets are redistributed."
                            </p>
                        </section>

                        <section>
                            <h3>"Figure Data"</h3>
                            <p>
                                "Names, wiki pages, and figure metadata are derived from the "
                                <a href="https://skylanders.fandom.com" target="_blank" rel="noopener">
                                    "Skylanders Fandom Wiki"
                                </a>
                                " and licensed under "
                                <a href="https://creativecommons.org/licenses/by-sa/3.0/" target="_blank" rel="noopener">
                                    "CC BY-SA 3.0"
                                </a>
                                ". Individual source pages are cited per-figure in the app's \
                                 bundled data."
                            </p>
                        </section>

                        <section>
                            <h3>"Fonts"</h3>
                            <p>
                                "Display and body typefaces are both licensed under the "
                                <a href="https://openfontlicense.org" target="_blank" rel="noopener">
                                    "SIL Open Font License 1.1"
                                </a>
                                "."
                            </p>
                            <ul>
                                <li>"Titan One © 2012 The Titan One Project Authors."</li>
                                <li>"Fraunces © 2019–2022 The Fraunces Project Authors."</li>
                            </ul>
                        </section>

                        <section>
                            <h3>".sky Tag Format"</h3>
                            <p>
                                "Mifare Classic tag layout + decryption logic is based on "
                                <a href="https://marijnkneppers.dev/posts/reverse-engineering-skylanders-toys-to-life-mechanics/" target="_blank" rel="noopener">
                                    "Marijn Kneppers' reverse-engineering write-up"
                                </a>
                                " (MIT-licensed sample code in the post's appendices)."
                            </p>
                        </section>
                    </div>
                    <div class="credits-actions">
                        <button class="credits-btn" type="button" on:click=close>"CLOSE"</button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
