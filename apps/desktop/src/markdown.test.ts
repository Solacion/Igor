import { describe, expect, it } from 'vitest'
import { renderMarkdown } from './markdown'

describe('renderMarkdown', () => {
  it('renders tables, emphasis and lists', () => {
    const html = renderMarkdown('Sales this week:\n\n| Product | Sales |\n|---|--:|\n| Melk 2L | R120 |\n\n- **Up** 4%\n- Down')
    expect(html).toContain('<table>')
    expect(html).toContain('<th style="text-align:right">Sales</th>')
    expect(html).toContain('<td>Melk 2L</td>')
    expect(html).toContain('<strong>Up</strong>')
    expect(html).toContain('<li>Down</li>')
  })

  it('escapes HTML and renders no links or images', () => {
    const html = renderMarkdown('<img src=x onerror=alert(1)> <script>x</script> [site](https://example.com) ![p](https://example.com/p.png) https://example.com')
    expect(html).not.toMatch(/<(img|script|a)\b/)
    expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;')
    expect(html).toContain('[site](https://example.com)')
  })
})
