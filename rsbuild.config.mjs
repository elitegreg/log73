import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

export default defineConfig({
  plugins: [pluginReact()],
  html: {
    title: 'Log73',
  },
  server: {
    historyApiFallback: true,
    proxy: {
      '/api': 'http://127.0.0.1:7300',
      '/ws': {
        target: 'ws://127.0.0.1:7300',
        ws: true,
      },
    },
  },
});
