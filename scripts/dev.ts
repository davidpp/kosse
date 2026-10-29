import process from "node:process";
// Build and run the native menu-bar app. Closing this command closes its owned window.
const build = Bun.spawn(["cargo", "build", "--locked", "-p", "kosse"], {
  stdio: ["inherit", "inherit", "inherit"],
});
if ((await build.exited) !== 0) process.exit(1);
const app = Bun.spawn(["cargo", "run", "--locked", "-p", "kosse", "--", ...process.argv.slice(2)], {
  stdio: ["inherit", "inherit", "inherit"],
});
for (const signal of ["SIGINT", "SIGTERM"] as const) process.on(signal, () => app.kill(signal));
process.exit(await app.exited);
