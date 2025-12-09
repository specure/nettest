import bonjour from 'bonjour';

/**
 * Example TypeScript client for discovering nettest servers via Bonjour/mDNS
 * 
 * This example demonstrates how to:
 * 1. Browse for _nettest._tcp services in the local network
 * 2. Parse TXT records to get server configuration
 * 3. Connect to discovered servers
 */

interface NettestServerConfig {
  tcp_port?: string;
  tls_port?: string;
  version?: string;
  server_name?: string;
  hostname: string;
  port: number;
  addresses: string[];
}

class NettestServiceDiscovery {
  private bonjourInstance: bonjour.Bonjour;
  private browser?: bonjour.Browser;
  private discoveredServers: Map<string, NettestServerConfig> = new Map();

  constructor() {
    this.bonjourInstance = bonjour();
  }

  /**
   * Start browsing for nettest services
   */
  startDiscovery(): void {
    console.log('🔍 Starting mDNS/Bonjour service discovery...');
    console.log('Looking for services: _nettest._tcp.local\n');

    // Browse for _nettest._tcp services
    // Note: bonjour library expects format without underscores and .local suffix
    // It will automatically add _ prefix and .local suffix
    // tcp or tls
    this.browser = this.bonjourInstance.find({ type: 'nettest', protocol: 'tcp' });

    // Handle service discovery
    this.browser.on('up', (service: bonjour.RemoteService) => {
      this.onServiceUp(service);
    });

    // Handle service removal
    this.browser.on('down', (service: bonjour.RemoteService) => {
      this.onServiceDown(service);
    });
  }

  /**
   * Handle when a service is discovered
   */
  private onServiceUp(service: bonjour.RemoteService): void {
    console.log('✅ Service discovered:');
    console.log(`   Name: ${service.name}`);
    console.log(`   Type: ${service.type}`);
    console.log(`   Host: ${service.host}`);
    console.log(`   Port: ${service.port}`);
    console.log(`   Addresses: ${service.addresses?.join(', ') || 'N/A'}`);
    
    // Parse TXT records
    const txt = service.txt || {};
    console.log(`   TXT Records:`);
    Object.entries(txt).forEach(([key, value]) => {
      console.log(`     ${key}: ${value}`);
    });

    // Build server configuration
    const config: NettestServerConfig = {
      hostname: service.host,
      port: service.port,
      addresses: service.addresses || [],
      tcp_port: txt['tcp_port'] as string | undefined,
      tls_port: txt['tls_port'] as string | undefined,
      version: txt['version'] as string | undefined,
      server_name: txt['server_name'] as string | undefined,
    };

    // Store discovered server
    const serviceKey = `${service.host}:${service.port}`;
    this.discoveredServers.set(serviceKey, config);

    console.log(`\n📋 Server Configuration:`);
    console.log(`   TCP Port: ${config.tcp_port || 'N/A'}`);
    console.log(`   TLS Port: ${config.tls_port || 'N/A'}`);
    console.log(`   Version: ${config.version || 'N/A'}`);
    console.log(`\n🔗 Connection URL: tcp://${config.hostname}:${config.tcp_port || config.port}\n`);
    console.log('─'.repeat(60) + '\n');

    // Example: You can now connect to the server
    this.connectToServer(config);
  }

  /**
   * Handle when a service goes down
   */
  private onServiceDown(service: bonjour.RemoteService): void {
    console.log('❌ Service removed:');
    console.log(`   Name: ${service.name}`);
    console.log(`   Host: ${service.host}:${service.port}\n`);

    const serviceKey = `${service.host}:${service.port}`;
    this.discoveredServers.delete(serviceKey);
  }

  /**
   * Example method to connect to discovered server
   */
  private connectToServer(config: NettestServerConfig): void {
    // This is just an example - implement your actual connection logic here
    console.log(`💡 Example: Ready to connect to server at ${config.hostname}:${config.tcp_port || config.port}`);
    
    // Example connection code (commented out):
    // const net = require('net');
    // const socket = net.createConnection({
    //   host: config.hostname,
    //   port: parseInt(config.tcp_port || String(config.port))
    // });
    // socket.on('connect', () => {
    //   console.log('Connected to server!');
    // });
  }

  /**
   * Get all currently discovered servers
   */
  getDiscoveredServers(): NettestServerConfig[] {
    return Array.from(this.discoveredServers.values());
  }

  /**
   * Stop discovery
   */
  stop(): void {
    console.log('\n🛑 Stopping service discovery...');
    if (this.browser) {
      this.browser.stop();
    }
    this.bonjourInstance.destroy();
    console.log('Service discovery stopped.');
  }
}

// Main execution
function main() {
  const discovery = new NettestServiceDiscovery();

  // Start discovery
  discovery.startDiscovery();

  // Handle graceful shutdown
  process.on('SIGINT', () => {
    console.log('\n\nReceived SIGINT, shutting down gracefully...');
    discovery.stop();
    process.exit(0);
  });

  process.on('SIGTERM', () => {
    console.log('\n\nReceived SIGTERM, shutting down gracefully...');
    discovery.stop();
    process.exit(0);
  });

  // Keep the process alive
  console.log('Discovery is running. Press Ctrl+C to stop.\n');
}

// Run if this file is executed directly
if (require.main === module) {
  main();
}

export { NettestServiceDiscovery, NettestServerConfig };

