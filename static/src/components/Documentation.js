import React, { useState } from 'react';
import './Documentation.css';

const Documentation = ({ isOpen, onClose }) => {
  const [activeSection, setActiveSection] = useState('overview');
  const [activeSubSection, setActiveSubSection] = useState('');

  const sections = {
    overview: {
      title: 'Overview',
      content: (
        <div>
          <h2>Overview</h2>
          <p>Nettest is a high-performance server and client for network speed measurement, written in Rust. The tool supports modern communication protocols and provides real-time accurate measurements.</p>
          
          <h3>Key Features</h3>
          
          <h4>🌐 Multi-Protocol Support</h4>
          <ul>
            <li><strong>TCP connections</strong> - Direct connection for maximum performance</li>
            <li><strong>WebSocket</strong> - Browser client support</li>
            <li><strong>TLS/SSL</strong> - Secure connections</li>
          </ul>
          
          <h4>⚡ High Performance</h4>
          <ul>
            <li><strong>Multithreading</strong> - Handle multiple clients simultaneously</li>
            <li><strong>Asynchronous architecture</strong> - Efficient resource utilization</li>
            <li><strong>Connection queue</strong> - Smart load distribution between workers</li>
          </ul>
          
          <h4>📊 Data Visualization</h4>
          <ul>
            <li><strong>Time speed change graphs</strong></li>
            <li><strong>Detailed measurement statistics</strong></li>
          </ul>
          
          <h4>🔧 Flexible Configuration</h4>
          <ul>
            <li>Configurable number of workers</li>
            <li>Configurable ports and addresses</li>
            <li>SSL/TLS certificate support</li>
          </ul>
        </div>
      )
    },
    server: {
      title: 'Server',
      subsections: {
        'how-to-run': {
          title: 'How to Run',
          content: (
            <div>
              <h3>How to Run Server</h3>
              <h4>Download</h4>
              <p>Download the latest builds directly:</p>
              
              <h5>Ubuntu</h5>
              <ul>
                <li><strong>Ubuntu 24.04 x86_64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-24-x86_64.tar.gz">nettest-ubuntu-24-x86_64.tar.gz</a></li>
                <li><strong>Ubuntu 24.04 ARM64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-24-aarch64.tar.gz">nettest-ubuntu-24-aarch64.tar.gz</a></li>
                <li><strong>Ubuntu 22.04 x86_64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-22-x86_64.tar.gz">nettest-ubuntu-22-x86_64.tar.gz</a></li>
                <li><strong>Ubuntu 22.04 ARM64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-22-aarch64.tar.gz">nettest-ubuntu-22-aarch64.tar.gz</a></li>
              </ul>
              
              <h5>Debian</h5>
              <ul>
                <li><strong>Debian 12 (Bookworm) x86_64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest-debian-12/nettest-debian-12-x86_64.tar.gz">nettest-debian-12-x86_64.tar.gz</a></li>
                <li><strong>Debian 12 (Bookworm) ARM64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest-debian-12/nettest-debian-12-aarch64.tar.gz">nettest-debian-12-aarch64.tar.gz</a></li>
                <li><strong>Debian 11 (Bullseye) x86_64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest-debian-11/nettest-debian-11-x86_64.tar.gz">nettest-debian-11-x86_64.tar.gz</a></li>
                <li><strong>Debian 11 (Bullseye) ARM64</strong>: <a href="https://github.com/specure/nettest/releases/download/latest-debian-11/nettest-debian-11-aarch64.tar.gz">nettest-debian-11-aarch64.tar.gz</a></li>
              </ul>
              
              <h5>macOS</h5>
              <ul>
                <li><strong>macOS Apple Silicon</strong>: <a href="https://github.com/specure/nettest/releases/download/latest-macos/nettest-macos-aarch64.tar.gz">nettest-macos-aarch64.tar.gz</a></li>
                <li><strong>macOS Intel</strong>: <a href="https://github.com/specure/nettest/releases/download/latest-macos/nettest-macos-x86_64.tar.gz">nettest-macos-x86_64.tar.gz</a></li>
              </ul>
              
              <div className="note">
                <strong>Note:</strong>
                <ol>
                  <li>Download the appropriate archive for your architecture and distribution</li>
                  <li>Extract: <code>tar -xzf nettest-&lt;distribution&gt;-&lt;arch&gt;.tar.gz</code></li>
                  <li>Run: <code>./nettest -s</code> (server) or <code>./nettest -c &lt;address&gt;</code> (client)</li>
                </ol>
              </div>
              
              <h4>Build</h4>
              <h5>Local Build</h5>
              <pre><code># Debug build
cargo build

# Release build with optimizations
cargo build --release</code></pre>
              
              <h5>GitHub Actions</h5>
              <p>The project includes automated builds via GitHub Actions:</p>
              <ul>
                <li><strong>Ubuntu builds</strong>: Latest and LTS versions with native compilation</li>
                <li><strong>Debian builds</strong>: Multiple versions (11, 12) for maximum compatibility</li>
                <li><strong>macOS builds</strong>: Apple Silicon and Intel architectures</li>
              </ul>
              
              <h4>Run Server</h4>
              <pre><code># Basic run
nettest -s</code></pre>
            </div>
          )
        },
        'ws': {
          title: 'WebSocket',
          content: (
            <div>
              <h3>WebSocket Mode</h3>
              <p>Browser client support:</p>
              <pre><code>Client &lt;──WebSocket──&gt; Server</code></pre>
              
              <h4>Configuration</h4>
              <p>WebSocket support is built into the server and can be used with browser-based clients like this application.</p>
              
              <h4>Usage</h4>
              <p>WebSocket connections are automatically handled by the server and provide real-time communication for speed measurements.</p>
            </div>
          )
        },
        'tls': {
          title: 'TLS',
          content: (
            <div>
              <h3>TLS Mode</h3>
              <p>Secure connections:</p>
              <pre><code>Client &lt;──TLS──&gt; Server</code></pre>
              
              <h4>Server Parameters</h4>
              <table>
                <thead>
                  <tr>
                    <th>Parameter</th>
                    <th>Description</th>
                    <th>Default</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td><code>-L</code></td>
                    <td>TLS listen address and port</td>
                    <td><code>443</code></td>
                  </tr>
                  <tr>
                    <td><code>-c</code></td>
                    <td>Path to SSL certificate (PEM format)</td>
                    <td>-</td>
                  </tr>
                  <tr>
                    <td><code>-k</code></td>
                    <td>Path to SSL key file (PEM format)</td>
                    <td>-</td>
                  </tr>
                </tbody>
              </table>
              
              <h4>Client Usage</h4>
              <pre><code># TLS client 
nettest -c &lt;SERVER_ADDRESS&gt; -tls</code></pre>
            </div>
          )
        },
        'auto-registration': {
          title: 'Auto Registration',
          content: (
            <div>
              <h3>Auto Registration</h3>
              <p>Nettest supports automatic server registration for easy discovery and management.</p>
              
              <h4>Features</h4>
              <ul>
                <li><strong>Automatic discovery</strong> - Servers can register themselves with control servers</li>
                <li><strong>Health monitoring</strong> - Regular status updates</li>
                <li><strong>Load balancing</strong> - Automatic distribution of clients</li>
              </ul>
              
              <h4>Configuration</h4>
              <p>Auto registration can be configured through environment variables or configuration files.</p>
            </div>
          )
        }
      }
    },
    client: {
      title: 'Client',
      content: (
        <div>
          <h3>Client Usage</h3>
          
          <h4>Run Client</h4>
          <pre><code># TCP client
nettest -c &lt;SERVER_ADDRESS&gt;

# WebSocket client
nettest -c &lt;SERVER_ADDRESS&gt; -ws

# TLS client 
nettest -c &lt;SERVER_ADDRESS&gt; -tls</code></pre>
          
          <h4>Client Parameters</h4>
          <table>
            <thead>
              <tr>
                <th>Parameter</th>
                <th>Description</th>
                <th>Default</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td><code>-c</code></td>
                <td>Server address</td>
                <td><code>127.0.0.1</code></td>
              </tr>
              <tr>
                <td><code>-tls</code></td>
                <td>Use TLS connection</td>
                <td><code>false</code></td>
              </tr>
              <tr>
                <td><code>-ws</code></td>
                <td>Use WebSocket connection</td>
                <td><code>false</code></td>
              </tr>
              <tr>
                <td><code>-t</code></td>
                <td>Number of threads</td>
                <td><code>3</code></td>
              </tr>
              <tr>
                <td><code>-p</code></td>
                <td>Port number</td>
                <td><code>8080</code></td>
              </tr>
              <tr>
                <td><code>-g</code></td>
                <td>Generate graphs</td>
                <td><code>false</code></td>
              </tr>
              <tr>
                <td><code>-log</code></td>
                <td>Log level (info, debug, trace)</td>
                <td>-</td>
              </tr>
            </tbody>
          </table>
        </div>
      )
    }
  };

  const renderContent = () => {
    const section = sections[activeSection];
    if (!section) return null;

    if (activeSection === 'server' && activeSubSection) {
      const subSection = section.subsections[activeSubSection];
      return subSection ? subSection.content : null;
    }

    return section.content;
  };

  if (!isOpen) return null;

  return (
    <div className="documentation-overlay">
      <div className="documentation-modal">
        <div className="documentation-header">
          <h2>Documentation</h2>
          <button className="close-button" onClick={onClose}>×</button>
        </div>
        
        <div className="documentation-content">
          <div className="documentation-sidebar">
            <nav className="documentation-nav">
              {Object.entries(sections).map(([key, section]) => (
                <div key={key} className="nav-section">
                  <button
                    className={`nav-item ${activeSection === key ? 'active' : ''}`}
                    onClick={() => {
                      setActiveSection(key);
                      setActiveSubSection('');
                    }}
                  >
                    {section.title}
                  </button>
                  
                  {section.subsections && activeSection === key && (
                    <div className="nav-subsections">
                      {Object.entries(section.subsections).map(([subKey, subSection]) => (
                        <button
                          key={subKey}
                          className={`nav-subitem ${activeSubSection === subKey ? 'active' : ''}`}
                          onClick={() => setActiveSubSection(subKey)}
                        >
                          {subSection.title}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </nav>
          </div>
          
          <div className="documentation-main">
            <div className="documentation-text">
              {renderContent()}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default Documentation; 