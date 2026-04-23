import { createApp } from "vue";

import App from "./App.vue";
import router from "./router";
import "./styles/tokens.css";
import "@opentiny/vue-theme/index.css";

createApp(App).use(router).mount("#app");
