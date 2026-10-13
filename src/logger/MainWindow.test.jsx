import React from 'react';
import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import { renderToString } from 'react-dom/server';
import MainWindow from './MainWindow.jsx';

test('MainWindow renders successfully', () => {
  const result = renderToString(<MainWindow />);
  assert.ok(result.includes('window'));
});
