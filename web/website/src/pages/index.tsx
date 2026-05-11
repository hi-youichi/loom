import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

function HomepageHeader() {
  const { siteConfig } = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">{siteConfig.tagline}</p>
        <div className={styles.buttons}>
          <Link className="button button--secondary button--lg" to="/docs/getting-started/overview">
            阅读文档
          </Link>
          <Link className="button button--outline button--lg" to="/docs/getting-started/quickstart">
            5 分钟快速入门
          </Link>
        </div>
      </div>
    </header>
  );
}

function Features() {
  const features = [
    {
      title: 'State Graph',
      description: '基于状态图的核心抽象，条件边、中间件、检查点一应俱全',
    },
    {
      title: '多运行模式',
      description: 'ReAct / DUP / ToT / GoT — 覆盖从简单到复杂的推理策略',
    },
    {
      title: '工具系统',
      description: '可插拔的 ToolSource：MCP、Web、Bash、Store 等开箱即用',
    },
    {
      title: 'LLM 集成',
      description: 'LlmClient trait 抽象，OpenAI / 兼容模型 / Mock 灵活切换',
    },
    {
      title: '记忆与持久化',
      description: 'Checkpointer + Store + Channels，支持 SQLite 持久化',
    },
    {
      title: '流式输出',
      description: 'StreamEvent 实时推送，WebSocket 多客户端连接',
    },
  ];

  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {features.map((f, idx) => (
            <div key={idx} className={clsx('col col--4', styles.feature)}>
              <h3>{f.title}</h3>
              <p>{f.description}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

export default function Home() {
  const { siteConfig } = useDocusaurusContext();
  return (
    <Layout title={`${siteConfig.title} - ${siteConfig.tagline}`} description="Loom 是一个基于 Rust 的图智能代理框架">
      <HomepageHeader />
      <main>
        <Features />
      </main>
    </Layout>
  );
}
