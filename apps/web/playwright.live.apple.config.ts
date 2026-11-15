import { defineConfig, devices } from '@playwright/test';

import base from './playwright.live.config.ts';

/** The same fresh-server onboarding and live scenarios, through Safari's engine. */
export default defineConfig({
  ...base,
  outputDir: 'test-results/live-apple',
  projects: base.projects?.map((project) => ({
    ...project,
    // The project's own address stays: the organizations and verifying servers are others.
    use: { ...project.use, ...devices['Desktop Safari'], deviceScaleFactor: 2 },
  })),
});
