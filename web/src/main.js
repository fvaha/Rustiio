import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'

export default mount(App, { target: document.getElementById('app') })

// Zamjenski ekran je odradio svoje — skloni ga da ne stoji iznad sučelja.
document.getElementById('boot')?.remove()
