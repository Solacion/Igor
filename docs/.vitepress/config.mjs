import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'IGOR',
  description: 'IGOR specifications, planning and user guides',
  rewrites: {
    'README.md': 'index.md',
    'specs/contracts/README.md': 'specs/contracts/index.md',
    'user-guides/README.md': 'user-guides/index.md'
  },
  themeConfig: {
    nav: [{ text: 'Documentation', link: '/' }],
    sidebar: [
      { text: 'Overview', link: '/' },
      {
        text: 'Specifications',
        items: [
          { text: 'Product and demo', link: '/specs/igor' },
          { text: 'Architecture', link: '/specs/architecture' },
          { text: 'Decisions', link: '/specs/decisions' },
          { text: 'Tool contracts', link: '/specs/contracts/' },
          { text: 'Sales and stock contract', link: '/specs/contracts/sales' },
          { text: 'Supplier catalogue contract', link: '/specs/contracts/supplier' },
          { text: 'Fixture file format', link: '/specs/contracts/fixture-format' }
        ]
      },
      { text: 'Delivery plan', link: '/planning/delivery-plan' },
      { text: 'Review standards', link: '/development/review-standards' },
      { text: 'Workspace', link: '/development/workspace' },
      {
        text: 'User guide',
        items: [
          { text: 'Overview', link: '/user-guides/' },
          { text: 'Configuration', link: '/user-guides/configuration' },
          { text: 'Verify a checkout', link: '/user-guides/verification' },
          { text: 'Evidence template', link: '/verification/run-template' }
        ]
      }
    ],
    search: { provider: 'local' }
  }
})
