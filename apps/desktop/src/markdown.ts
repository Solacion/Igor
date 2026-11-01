import MarkdownIt from 'markdown-it'

/**
 * Renders Igor's replies: paragraphs, emphasis, lists, tables and code. Model text is untrusted,
 * so raw HTML is escaped, and links and images are not rendered: a link could navigate the app
 * window and an image could fetch a remote URL. Their text stays visible.
 */
const md = new MarkdownIt({ html: false, linkify: false, typographer: false, breaks: true })
  .disable(['link', 'image', 'autolink', 'html_inline', 'html_block', 'reference'])

/** HTML for a reply written in Markdown. Safe to bind with `v-html`. */
export function renderMarkdown(text: string): string {
  return md.render(text)
}
