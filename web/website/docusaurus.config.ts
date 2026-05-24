import type { Config } from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Loom',
  tagline: 'Graph-based Agent Framework in Rust',
  favicon: 'img/favicon.ico',

  url: 'https://your-docusaurus-site.example.com',
  baseUrl: '/',

  onBrokenLinks: 'warn',

  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },

  i18n: {
    defaultLocale: 'zh-CN',
    locales: ['zh-CN'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          path: '../../docs',
          routeBasePath: 'docs',
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/anthropics/loom/tree/main/docs',
          exclude: [
            'dev/**',
            'rfcs/**',
            'DOC-PLAN.md',
          ],
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/loom-social-card.jpg',
    navbar: {
      title: 'Loom',
      logo: {
        alt: 'Loom Logo',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: '文档',
        },
        {
          href: 'https://github.com/anthropics/loom',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: '文档',
          items: [
            { label: '概览', to: '/docs/getting-started/overview' },
            { label: '快速入门', to: '/docs/getting-started/quickstart' },
            { label: '核心概念', to: '/docs/getting-started/concepts' },
          ],
        },
        {
          title: '社区',
          items: [
            { label: 'GitHub', href: 'https://github.com/anthropics/loom' },
          ],
        },
        {
          title: '更多',
          items: [
            { label: 'API 参考', to: '/docs/advanced/api-reference' },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Loom. Built with Docusaurus.`,
    },
    prism: {
      additionalLanguages: ['rust', 'toml', 'bash', 'json'],
    },
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: false,
      respectPrefersColorScheme: true,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
