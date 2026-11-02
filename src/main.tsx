import React from 'react';
import { createRoot } from 'react-dom/client';
import './index.css';
import { App } from './App';

const rootEl = document.getElementById('root') || document.getElementById('app');
if (rootEl) {
  createRoot(rootEl).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
}
