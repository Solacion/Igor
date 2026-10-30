import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'IGOR',
  description: 'IGOR specifications, planning and user guides',
  rewrites: {
    'README.md': 'index.md',
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
          { text: 'Decisions', link: '/specs/decisions' }
        ]
      },
      { text: 'Delivery plan', link: '/planning/delivery-plan' },
      { text: 'Review standards', link: '/development/review-standards' },
      { text: 'Workspace', link: '/development/workspace' },
      { text: 'User guide', link: '/user-guides/' }
    ],
    search: { provider: 'local' }
  }
})
