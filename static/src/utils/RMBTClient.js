import TestVisualizationService from './TestVisualizationService';

class RMBTClient {
  constructor(serverUrl, serverId) {
    this.serverUrl = serverUrl;
    this.serverId = serverId;
    this.rmbtws = null;
    this.test = null;
    this.visualizationService = new TestVisualizationService();
    this.connected = false;
    this.results = {
      download: null,
      upload: null,
      ping: null,
      jitter: null
    };
  }

  async init() {
    try {
      // Динамический импорт rmbtws как в portal
      const rmbtws = await import('rmbtws');
      this.rmbtws = rmbtws;
      
      // Инициализируем TestEnvironment с нашим сервисом как в portal
      this.rmbtws.TestEnvironment.init(this.visualizationService, null);
      
      // Создаем конфигурацию теста как в portal, но используем локальный прокси
      const config = new this.rmbtws.RMBTTestConfig(
        'en',
        'http://localhost:3001/api/proxy',
        ''
      );
      
      // Устанавливаем UUID и другие параметры как в portal
      config.uuid = this.generateUUID();
      config.timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
      config.additionalSubmissionParameters = { network_type: 0 };
      config.additionalRegistrationParameters = {
        uuid_permission_granted: true,
        app_version: '1.0.0',
      };

      // Создаем сервер для тестирования с правильными параметрами
      const testServer = {
        id: this.serverId, // Используем переданный ID сервера
        name: 'Test Server',
        webAddress: this.serverUrl,
        serverType: 'RMBTws',
        port: 443,
        port_ssl: 443,
        sponsor: 'Test',
        country: 'Test',
        address: this.serverUrl,
        distance: 0,
        city: 'Test'
      };

      // Создаем коммуникацию с сервером с правильными заголовками как в portal
      const ctrl = new this.rmbtws.RMBTControlServerCommunication(
        config,
        {
          'Content-Type': 'application/json',
          'X-Nettest-Client': 'nt',
        },
        testServer
      );

      // Создаем тест
      this.test = new this.rmbtws.RMBTTest(config, ctrl);
      
      // Устанавливаем тест в TestEnvironment как в portal
      this.rmbtws.TestEnvironment.getTestVisualization().setRMBTTest(this.test);

      // Устанавливаем обработчики событий
      this.setupEventHandlers();

      return true;
    } catch (error) {
      console.error('Failed to initialize RMBT client:', error);
      throw error;
    }
  }

  setupEventHandlers() {
    if (!this.test) return;

    // Устанавливаем обработчики в TestVisualizationService
    this.visualizationService.setOnProgress((result) => {
      console.log('Test progress:', result);
    });

    this.visualizationService.setOnComplete((result) => {
      console.log('Test completed:', result);
      this.results = {
        download: result.downloadSpeed || null,
        upload: result.uploadSpeed || null,
        ping: result.ping || null,
        jitter: result.jitter || null
      };
      this.connected = false;
    });

    this.visualizationService.setOnError((error) => {
      console.error('Test error:', error);
      this.connected = false;
    });

    // Обработчик для обновления графика пинга
    this.visualizationService.setOnPingUpdate((pingData) => {
      if (this.onPingUpdate) {
        this.onPingUpdate(pingData);
      }
    });

    // Обработчик для обновления графика download
    this.visualizationService.setOnDownloadUpdate((downloadData) => {
      if (this.onDownloadUpdate) {
        this.onDownloadUpdate(downloadData);
      }
    });

    // Обработчик для обновления графика upload
    this.visualizationService.setOnUploadUpdate((uploadData) => {
      if (this.onUploadUpdate) {
        this.onUploadUpdate(uploadData);
      }
    });

    // Обработчик для обновления pingMedian
    this.visualizationService.setOnPingMedianUpdate((pingMedian) => {
      console.log('Ping median update:', pingMedian);
      if (this.onPingMedianUpdate) {
        this.onPingMedianUpdate(pingMedian);
      }
    });

    // Обработчик для обновления фазы теста
    this.visualizationService.setOnPhaseUpdate((phase, result) => {
      console.log('Phase update:', phase, result);
      if (this.onPhaseUpdate) {
        this.onPhaseUpdate(phase, result);
      }
    });

    // Добавляем обработчик ошибок для API
    if (this.test && typeof this.test.on === 'function') {
      this.test.on('error', (error) => {
        console.error('RMBT test error:', error);
      });
    }
  }

  // Метод для установки callback для обновления графика пинга
  setOnPingUpdate(callback) {
    this.onPingUpdate = callback;
  }

  // Метод для установки callback для обновления графика download
  setOnDownloadUpdate(callback) {
    this.onDownloadUpdate = callback;
  }

  // Метод для установки callback для обновления графика upload
  setOnUploadUpdate(callback) {
    this.onUploadUpdate = callback;
  }

  // Метод для установки callback для обновления pingMedian
  setOnPingMedianUpdate(callback) {
    this.onPingMedianUpdate = callback;
  }

  // Метод для установки callback для обновления фазы теста
  setOnPhaseUpdate(callback) {
    this.onPhaseUpdate = callback;
  }

  generateUUID() {
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function(c) {
      const r = Math.random() * 16 | 0;
      const v = c === 'x' ? r : (r & 0x3 | 0x8);
      return v.toString(16);
    });
  }

  async connect() {
    try {
      await this.init();
      console.log('RMBT client initialized');
      return Promise.resolve();
    } catch (error) {
      console.error('Failed to connect:', error);
      return Promise.reject(error);
    }
  }

  async runPingTest() {
    return new Promise((resolve, reject) => {
      if (!this.test) {
        reject(new Error('Test not initialized'));
        return;
      }

      try {
        // Запускаем тест как в portal
        this.test.startTest();
        this.rmbtws.TestEnvironment.getTestVisualization().startTest();
        
        // Ждем реальных результатов
        let attempts = 0;
        const maxAttempts = 100; // 10 секунд максимум
        
        const checkResults = setInterval(() => {
          attempts++;
          
          const testResult = this.visualizationService.getTestResult();
          if (testResult && testResult.status === 'END') {
            clearInterval(checkResults);
            resolve({
              ping: testResult.ping || 0,
              jitter: testResult.jitter || 0
            });
          } else if (attempts >= maxAttempts) {
            clearInterval(checkResults);
            reject(new Error('Ping test timeout'));
          }
        }, 100);
      } catch (error) {
        reject(error);
      }
    });
  }

  async runDownloadTest() {
    return new Promise((resolve, reject) => {
      if (!this.test) {
        reject(new Error('Test not initialized'));
        return;
      }

      try {
        // Запускаем тест как в portal
        this.test.startTest();
        this.rmbtws.TestEnvironment.getTestVisualization().startTest();
        
        // Ждем реальных результатов
        let attempts = 0;
        const maxAttempts = 200; // 20 секунд максимум
        
        const checkResults = setInterval(() => {
          attempts++;
          
          const testResult = this.visualizationService.getTestResult();
          if (testResult && testResult.status === 'END') {
            clearInterval(checkResults);
            resolve({
              speed: testResult.downloadSpeed || 0
            });
          } else if (attempts >= maxAttempts) {
            clearInterval(checkResults);
            reject(new Error('Download test timeout'));
          }
        }, 100);
      } catch (error) {
        reject(error);
      }
    });
  }

  async runUploadTest() {
    return new Promise((resolve, reject) => {
      if (!this.test) {
        reject(new Error('Test not initialized'));
        return;
      }

      try {
        // Запускаем тест как в portal
        this.test.startTest();
        this.rmbtws.TestEnvironment.getTestVisualization().startTest();
        
        // Ждем реальных результатов
        let attempts = 0;
        const maxAttempts = 200; // 20 секунд максимум
        
        const checkResults = setInterval(() => {
          attempts++;
          
          const testResult = this.visualizationService.getTestResult();
          if (testResult && testResult.status === 'END') {
            clearInterval(checkResults);
            resolve({
              speed: testResult.uploadSpeed || 0
            });
          } else if (attempts >= maxAttempts) {
            clearInterval(checkResults);
            reject(new Error('Upload test timeout'));
          }
        }, 100);
      } catch (error) {
        reject(error);
      }
    });
  }

  async runFullTest() {
    try {
      console.log('=== RMBTClient.runFullTest ===');
      
      // Запускаем тест
      this.test.startTest();
      this.rmbtws.TestEnvironment.getTestVisualization().startTest();
      
      console.log('Test started, waiting for results...');
      
      // Ждем результаты с таймаутом
      const timeout = 60000; // 60 секунд
      const startTime = Date.now();
      
      while (Date.now() - startTime < timeout) {
        const result = this.visualizationService.getTestResult();
        
        if (result && result.status === 'END') {
          console.log('Test completed with result:', result);
          
          // Получаем вычисленную медиану пинга
          const pingMedian = this.visualizationService.getPingMedian();
          console.log('Final ping median from visualization service:', pingMedian);
          
          // Извлекаем финальные результаты
          const finalResults = {
            download: result.downloadSpeed || result.downBitPerSec ? (result.downloadSpeed || result.downBitPerSec) / 1000000 : null,
            upload: result.uploadSpeed || result.upBitPerSec ? (result.uploadSpeed || result.upBitPerSec) / 1000000 : null,
            ping: pingMedian, // Используем вычисленную медиану
            jitter: null // Убираем джиттер
          };
          
          console.log('Final results:', finalResults);
          this.results = finalResults;
          return finalResults;
        }
        
        if (result && (result.status === 'ERROR' || result.status === 'ABORTED')) {
          console.log('Test failed with result:', result);
          throw new Error(`Test failed: ${result.status}`);
        }
        
        // Ждем немного перед следующей проверкой
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      
      throw new Error('Test timeout');
    } catch (error) {
      console.error('Error in runFullTest:', error);
      throw error;
    }
  }

  disconnect() {
    if (this.rmbtws && this.test) {
      // Используем правильный API как в portal
      this.rmbtws.TestEnvironment.getTestVisualization()?.stopTest();
      this.test = null;
    }
    this.connected = false;
  }
}

export default RMBTClient; 