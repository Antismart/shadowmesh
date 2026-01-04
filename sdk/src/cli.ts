#!/usr/bin/env node
import { Command } from 'commander';
import chalk from 'chalk';
import { ShadowMesh } from './index.js';

const program = new Command();

program
  .name('shadowmesh')
  .description('Deploy censorship-resistant applications to ShadowMesh')
  .version('0.1.0');

program
  .command('deploy')
  .description('Deploy a file or directory to ShadowMesh')
  .argument('<path>', 'Path to file or directory')
  .option('-d, --domain <domain>', 'Custom domain name')
  .option('-e, --ens <name>', 'ENS domain name')
  .option('-p, --privacy <level>', 'Privacy level (low|medium|high)', 'medium')
  .option('-r, --redundancy <number>', 'Number of replicas', '5')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .action(async (path: string, options: {
    domain?: string;
    ens?: string;
    privacy?: 'low' | 'medium' | 'high';
    redundancy?: string;
    network?: 'testnet' | 'mainnet';
  }) => {
    try {
      const mesh = new ShadowMesh({ network: options.network || 'testnet' });
      
      console.log(chalk.blue('\n🌐 ShadowMesh Deployment\n'));
      
      const result = await mesh.deploy({
        path,
        domain: options.domain,
        ens: options.ens,
        privacy: options.privacy,
        redundancy: parseInt(options.redundancy || '5'),
      });

      console.log(chalk.green('\n✅ Deployment successful!\n'));
      console.log(chalk.cyan('Gateway URL:'), result.gateway);
      console.log(chalk.cyan('Native URL: '), result.native);
      console.log(chalk.cyan('Content ID: '), result.cid);
      
      if (result.ens) {
        console.log(chalk.cyan('ENS URL:    '), result.ens);
      }

      console.log(chalk.gray('\nManifest:'));
      console.log(chalk.gray(`  Hash: ${result.manifest.content_hash}`));
      console.log(chalk.gray(`  Fragments: ${result.manifest.fragments.length}`));
      console.log(chalk.gray(`  Size: ${result.manifest.metadata.size} bytes`));
      console.log();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Deployment failed:'), message);
      process.exit(1);
    }
  });

program
  .command('status')
  .description('Check deployment status')
  .argument('<cid>', 'Content ID')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .action(async (cid: string, options: { network?: 'testnet' | 'mainnet' }) => {
    try {
      const mesh = new ShadowMesh({ network: options.network || 'testnet' });
      
      console.log(chalk.blue(`\n🔍 Checking status for ${cid}...\n`));
      
      const status = await mesh.status(cid);
      
      if (status.available) {
        console.log(chalk.green('✅ Content is available'));
        console.log(chalk.cyan(`Replicas: ${status.replicas}`));
      } else {
        console.log(chalk.yellow('⚠️  Content not found or unavailable'));
      }
      console.log();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Status check failed:'), message);
      process.exit(1);
    }
  });

program
  .command('stats')
  .description('Show network statistics')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .action(async (options: { network?: 'testnet' | 'mainnet' }) => {
    try {
      const mesh = new ShadowMesh({ network: options.network || 'testnet' });
      
      console.log(chalk.blue('\n📊 ShadowMesh Network Statistics\n'));
      
      const stats = await mesh.networkStats();
      
      console.log(chalk.cyan('Active Nodes:    '), stats.nodes.toLocaleString());
      console.log(chalk.cyan('Total Bandwidth: '), `${(stats.bandwidth / 1024 / 1024 / 1024).toFixed(2)} GB`);
      console.log(chalk.cyan('Files Hosted:    '), stats.files.toLocaleString());
      console.log();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Failed to fetch stats:'), message);
      process.exit(1);
    }
  });

program
  .command('init')
  .description('Initialize a new ShadowMesh project')
  .action(() => {
    console.log(chalk.blue('\n🚀 Initializing ShadowMesh project...\n'));
    console.log(chalk.yellow('Coming soon: Project scaffolding'));
    console.log();
  });

program.parse();
