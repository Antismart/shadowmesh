#!/usr/bin/env node
import { Command } from 'commander';
import chalk from 'chalk';
import fs from 'fs/promises';
import path from 'path';
import { ShadowMesh, GatewayClient, NameResolver, FALLBACK_GATEWAY_URLS, formatBytes, formatDuration } from './index.js';

const program = new Command();

program
  .name('shadowmesh')
  .description('Deploy censorship-resistant applications to ShadowMesh')
  .version('0.1.0');

// ============================================================================
// Deploy Command
// ============================================================================

program
  .command('deploy')
  .description('Deploy a file or directory to ShadowMesh')
  .argument('<path>', 'Path to file or directory')
  .option('-d, --domain <domain>', 'Custom domain name')
  // ENS integration not yet implemented — removed --ens flag
  .option('-p, --privacy <level>', 'Privacy level (low|medium|high)', 'medium')
  .option('-r, --redundancy <number>', 'Number of replicas', '5')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .option('--encrypt', 'Encrypt content before upload')
  .option('--password <password>', 'Encryption password')
  .option('--pin', 'Pin content permanently')
  .option('--bootstrap <addrs>', 'Comma-separated bootstrap multiaddrs for DNS-free discovery')
  .option('--no-dns', 'Disable DNS-based fallback (pure P2P resolution only)')
  .action(async (filePath: string, options: {
    domain?: string;
    privacy?: 'low' | 'medium' | 'high';
    redundancy?: string;
    network?: 'testnet' | 'mainnet';
    encrypt?: boolean;
    password?: string;
    pin?: boolean;
  }) => {
    try {
      const mesh = new ShadowMesh({ network: options.network || 'testnet' });
      
      console.log(chalk.blue('\n🌐 ShadowMesh Deployment\n'));
      
      // Check if file exists
      const stats = await fs.stat(filePath);
      console.log(chalk.gray(`  File: ${path.basename(filePath)}`));
      console.log(chalk.gray(`  Size: ${formatBytes(stats.size)}`));
      console.log(chalk.gray(`  Type: ${stats.isDirectory() ? 'Directory' : 'File'}`));
      console.log();
      
      const result = await mesh.deploy({
        path: filePath,
        domain: options.domain,
        privacy: options.privacy,
        redundancy: parseInt(options.redundancy || '5'),
      });

      console.log(chalk.green('\n✅ Deployment successful!\n'));
      console.log(chalk.cyan('Gateway URL:'), result.gateway);
      console.log(chalk.cyan('Native URL: '), result.native);
      console.log(chalk.cyan('Content ID: '), result.cid);
      
      console.log(chalk.gray('\nManifest:'));
      console.log(chalk.gray(`  Hash: ${result.manifest.content_hash}`));
      console.log(chalk.gray(`  Fragments: ${result.manifest.fragments.length}`));
      console.log(chalk.gray(`  Size: ${formatBytes(result.manifest.metadata.size)}`));
      console.log();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Deployment failed:'), message);
      process.exit(1);
    }
  });

// ============================================================================
// Status Command
// ============================================================================

program
  .command('status')
  .description('Check deployment status')
  .argument('<cid>', 'Content ID')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .option('-v, --verbose', 'Show detailed information')
  .action(async (cid: string, options: { network?: 'testnet' | 'mainnet'; verbose?: boolean }) => {
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

// ============================================================================
// Get Command
// ============================================================================

program
  .command('get')
  .description('Download content from ShadowMesh')
  .argument('<cid>', 'Content ID')
  .option('-o, --output <path>', 'Output file path')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .option('--password <password>', 'Decryption password (if encrypted)')
  .action(async (cid: string, options: { 
    output?: string; 
    network?: 'testnet' | 'mainnet';
    password?: string;
  }) => {
    try {
      const gatewayUrl = options.network === 'mainnet'
        ? 'https://api.shadowmesh.network'
        : 'https://testnet.shadowmesh.network';
      
      const client = new GatewayClient({ gatewayUrl });
      
      console.log(chalk.blue(`\n📥 Downloading ${cid}...\n`));
      
      const startTime = Date.now();
      const content = await client.getContent(cid);
      const duration = Date.now() - startTime;
      
      const outputPath = options.output || `${cid.slice(0, 16)}.bin`;
      await fs.writeFile(outputPath, new Uint8Array(content));
      
      console.log(chalk.green('✅ Download complete!\n'));
      console.log(chalk.cyan('Output:   '), outputPath);
      console.log(chalk.cyan('Size:     '), formatBytes(content.byteLength));
      console.log(chalk.cyan('Duration: '), formatDuration(duration));
      console.log();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Download failed:'), message);
      process.exit(1);
    }
  });

// ============================================================================
// Stats Command
// ============================================================================

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
      console.log(chalk.cyan('Total Bandwidth: '), formatBytes(stats.bandwidth));
      console.log(chalk.cyan('Files Hosted:    '), stats.files.toLocaleString());
      console.log();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Failed to fetch stats:'), message);
      process.exit(1);
    }
  });

// ============================================================================
// Pin Command
// ============================================================================

program
  .command('pin')
  .description('Pin content to keep it available')
  .argument('<cid>', 'Content ID to pin')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .action(async (_cid: string, _options: { network?: 'testnet' | 'mainnet' }) => {
    console.error(chalk.red('❌ Pin command is not yet implemented.'));
    console.error(chalk.gray('   Content is pinned automatically during deployment.'));
    process.exit(1);
  });

// ============================================================================
// Unpin Command
// ============================================================================

program
  .command('unpin')
  .description('Unpin content')
  .argument('<cid>', 'Content ID to unpin')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .action(async (_cid: string, _options: { network?: 'testnet' | 'mainnet' }) => {
    console.error(chalk.red('❌ Unpin command is not yet implemented.'));
    process.exit(1);
  });

// ============================================================================
// List Command
// ============================================================================

program
  .command('list')
  .alias('ls')
  .description('List your deployed content')
  .option('-n, --network <network>', 'Network (testnet|mainnet)', 'testnet')
  .option('--limit <number>', 'Maximum number of items', '20')
  .action(async (_options: { network?: 'testnet' | 'mainnet'; limit?: string }) => {
    console.error(chalk.red('❌ List command is not yet implemented.'));
    console.error(chalk.gray('   Use the gateway dashboard to view deployments.'));
    process.exit(1);
  });

// ============================================================================
// Init Command
// ============================================================================

program
  .command('init')
  .description('Initialize a new ShadowMesh project')
  .option('--typescript', 'Use TypeScript')
  .option('--framework <name>', 'Framework to use (react|vue|svelte|vanilla)')
  .action(async (options: { typescript?: boolean; framework?: string }) => {
    try {
      console.log(chalk.blue('\n🚀 Initializing ShadowMesh project...\n'));
      
      // Create shadowmesh.config.json
      const config = {
        $schema: 'https://shadowmesh.network/schemas/config.json',
        network: 'testnet',
        privacy: 'medium',
        redundancy: 5,
        build: {
          outDir: 'dist',
          publicDir: 'public',
        },
        deploy: {
          pin: true,
          announce: true,
        },
      };
      
      await fs.writeFile(
        'shadowmesh.config.json',
        JSON.stringify(config, null, 2)
      );
      
      console.log(chalk.green('✅ Created shadowmesh.config.json'));
      console.log();
      console.log(chalk.gray('Next steps:'));
      console.log(chalk.gray('  1. Edit shadowmesh.config.json to configure your project'));
      console.log(chalk.gray('  2. Run `shadowmesh deploy <path>` to deploy your app'));
      console.log();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Initialization failed:'), message);
      process.exit(1);
    }
  });

// ============================================================================
// Config Command
// ============================================================================

program
  .command('config')
  .description('Manage ShadowMesh configuration')
  .option('--get <key>', 'Get a config value')
  .option('--set <key=value>', 'Set a config value')
  .option('--list', 'List all config values')
  .action(async (options: { get?: string; set?: string; list?: boolean }) => {
    try {
      const configPath = 'shadowmesh.config.json';
      
      if (options.list || (!options.get && !options.set)) {
        // List all config
        try {
          const content = await fs.readFile(configPath, 'utf-8');
          const config = JSON.parse(content);
          console.log(chalk.blue('\n⚙️  ShadowMesh Configuration\n'));
          console.log(JSON.stringify(config, null, 2));
          console.log();
        } catch {
          console.log(chalk.yellow('\nNo configuration file found.'));
          console.log(chalk.gray('Run `shadowmesh init` to create one.\n'));
        }
        return;
      }
      
      if (options.get) {
        const content = await fs.readFile(configPath, 'utf-8');
        const config = JSON.parse(content);
        const value = options.get.split('.').reduce((obj, key) => obj?.[key], config);
        console.log(value !== undefined ? value : chalk.yellow('Key not found'));
        return;
      }
      
      if (options.set) {
        const [key, value] = options.set.split('=');
        let content = '{}';
        try {
          content = await fs.readFile(configPath, 'utf-8');
        } catch {
          // File doesn't exist, start with empty config
        }
        const config = JSON.parse(content);
        
        // Set nested value
        const keys = key.split('.');
        let obj = config;
        for (let i = 0; i < keys.length - 1; i++) {
          if (!obj[keys[i]]) obj[keys[i]] = {};
          obj = obj[keys[i]];
        }
        obj[keys[keys.length - 1]] = isNaN(Number(value)) ? value : Number(value);
        
        await fs.writeFile(configPath, JSON.stringify(config, null, 2));
        console.log(chalk.green(`✅ Set ${key} = ${value}`));
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown error';
      console.error(chalk.red('❌ Config operation failed:'), message);
      process.exit(1);
    }
  });

// ============================================================================
// Version/Info
// ============================================================================

program
  .command('info')
  .description('Show ShadowMesh information')
  .action(() => {
    console.log(chalk.blue('\n🌐 ShadowMesh SDK\n'));
    console.log(chalk.cyan('Version:  '), '0.1.0');
    console.log(chalk.cyan('Homepage: '), 'https://shadowmesh.network');
    console.log(chalk.cyan('Docs:     '), 'https://docs.shadowmesh.network');
    console.log(chalk.cyan('GitHub:   '), 'https://github.com/shadowmesh');
    console.log();
    console.log(chalk.gray('Networks:'));
    console.log(chalk.gray('  Testnet: https://testnet.shadowmesh.network'));
    console.log(chalk.gray('  Mainnet: https://api.shadowmesh.network'));
    console.log();
  });

program.parse();
