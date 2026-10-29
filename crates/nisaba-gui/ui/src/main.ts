import { mount } from 'svelte';
import '@fontsource/cormorant-garamond/500.css';
import '@fontsource/ibm-plex-sans/400.css';
import '@fontsource/ibm-plex-sans/500.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/noto-sans-cuneiform/400.css';
import App from './App.svelte';
import './lib/tema.css';
mount(App, { target: document.getElementById('app')! });
