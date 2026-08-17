export function parseArgs(argv) {
  const args = { wait: 10000, timeout: 30000 };
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`missing value for ${key}`);
      return argv[++i];
    };
    switch (key) {
      case "--url": args.url = next(); break;
      case "--wait": args.wait = Number(next()); break;
      case "--after": args.after = Number(next()); break;
      case "--timeout": args.timeout = Number(next()); break;
      default: throw new Error(`unknown flag ${key}`);
    }
  }
  return args;
}
