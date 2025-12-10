import Bonjour, { Service } from 'bonjour-service';

/**
 * Example TypeScript client for querying nettest servers via Bonjour/mDNS
 * 
 * This example demonstrates how to:
 * 1. Query TXT records for _nettest._tcp services using DNS-SD queries
 * 2. Parse TXT records to get server configuration
 * 3. Connect to discovered servers
 * 
 * Equivalent to: dns-sd -Q "nettest._nettest._tcp.local" TXT
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
  private bonjourInstance: Bonjour;
  private browser?: any;
  private discoveredServers: Map<string, NettestServerConfig> = new Map();

  constructor() {
    this.bonjourInstance = new Bonjour({
        name: 'nettest',
    });
  }

  /**
   * Start querying for nettest services using DNS-SD
   * This is equivalent to: dns-sd -Q "nettest._nettest._tcp.local" TXT
   */
  startDiscovery(): void {
    console.log('🔍 Starting mDNS/Bonjour service discovery...');
    console.log('Querying for services: _nettest._tcp\n');

    // Browse for _nettest._tcp services
    // bonjour-service will automatically query TXT records
    this.browser = this.bonjourInstance.find({ type: 'nettest', protocol: 'tcp' });

    // Handle service discovery
    this.browser.on('up', (service: Service) => {
      this.onServiceUp(service);
    });

    // Handle service removal
    this.browser.on('down', (service: Service) => {
      this.onServiceDown(service);
    });

    // Start the browser
    this.browser.start();
  }

  /**
   * Handle when a service is discovered
   */
  private onServiceUp(service: Service): void {
    const serviceKey = `${service.host}:${service.port}`;
    
    console.log('✅ Service discovered:');
    console.log(`   Name: ${service.name}`);
    console.log(`   Type: ${service.type}`);
    console.log(`   Protocol: ${service.protocol}`);
    console.log(`   Host: ${service.host}`);
    console.log(`   Port: ${service.port}`);
    console.log(`   FQDN: ${service.fqdn}`);
    console.log(`   Addresses: ${service.addresses?.join(', ') || 'N/A'}`);
    
    // Parse TXT records
    const txt = service.txt || {};
    console.log(`   TXT Records:`);
    if (Object.keys(txt).length > 0) {
      Object.entries(txt).forEach(([key, value]) => {
        console.log(`     ${key}: ${value}`);
      });
    } else {
      console.log(`     (no TXT records)`);
    }

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

    // Store discovered server (update if exists)
    this.discoveredServers.set(serviceKey, config);

    console.log(`\n📋 Server Configuration:`);
    console.log(`   TCP Port: ${config.tcp_port || 'N/A'}`);
    console.log(`   TLS Port: ${config.tls_port || 'N/A'}`);
    console.log(`   Version: ${config.version || 'N/A'}`);
    console.log(`   Server Name: ${config.server_name || 'N/A'}`);
    console.log(`\n🔗 Connection URL: tcp://${config.hostname}:${config.tcp_port || config.port}\n`);
    console.log('─'.repeat(60) + '\n');

    // Example: You can now connect to the server
    this.connectToServer(config);
  }

  /**
   * Handle when a service goes down
   */
  private onServiceDown(service: Service): void {
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
