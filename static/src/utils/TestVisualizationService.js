class TestVisualizationService {
  constructor() {
    this.rmbtTest = null;
    this.testUUID = null;
    this.onProgress = null;
    this.onComplete = null;
    this.onError = null;
    this.testResult = null;
    this.pingData = [];
    this.downloadData = [];
    this.uploadData = [];
    this.startTime = null;
    this.onPingUpdate = null;
    this.onDownloadUpdate = null;
    this.onUploadUpdate = null;
    this.onPhaseUpdate = null;
    this.onPingMedianUpdate = null;
    this.redrawLoop = null;
    this.lastProgress = -1;
    this.lastStatus = -1;
    this.currentPhase = 'INIT';
    this.pingMedian = null;
    this.pingValues = []; // Массив для сбора значений пинга
  }

  // Метод для вычисления медианы
  calculateMedian(values) {
    if (values.length === 0) return null;
    
    const sorted = [...values].sort((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);
    
    if (sorted.length % 2 === 0) {
      return (sorted[middle - 1] + sorted[middle]) / 2;
    } else {
      return sorted[middle];
    }
  }

  setRMBTTest(rmbtTest) {
    this.rmbtTest = rmbtTest;
  }

  startTest() {
    console.log('Test visualization started');
    this.startTime = Date.now();
    this.pingData = [];
    this.downloadData = [];
    this.uploadData = [];
    this.pingValues = []; // Сбрасываем массив значений пинга
    this.lastProgress = -1;
    this.lastStatus = -1;
    this.currentPhase = 'INIT';
    this.draw();
  }

  stopTest() {
    console.log('Test visualization stopped');
    if (this.redrawLoop) {
      clearTimeout(this.redrawLoop);
      this.redrawLoop = null;
    }
  }

  updateInfo(serverName, remoteIp, providerName, testUUID) {
    this.testUUID = testUUID;
    console.log('Test info updated:', { serverName, remoteIp, providerName, testUUID });
  }

  // Основной метод для получения и обработки результатов (как в portal)
  draw = () => {
    const result = this.rmbtTest?.getIntermediateResult();
    const status = result?.status?.toString();

    console.log('=== TestVisualizationService.draw ===');
    console.log('Intermediate result:', result);
    console.log('Status:', status);
    console.log('Progress:', result?.progress);

    // Проверяем, нужно ли продолжать цикл
    if (
      result === null ||
      (result.progress === this.lastProgress &&
        this.lastProgress !== 1 &&
        this.lastStatus === status &&
        this.lastStatus !== 'QOS_TEST_RUNNING' &&
        this.lastStatus !== 'QOS_END' &&
        this.lastStatus !== 'SPEEDTEST_END')
    ) {
      this.redrawLoop = setTimeout(this.draw, 160);
      return;
    }

    this.lastProgress = result?.progress;
    this.lastStatus = status;

    // Обрабатываем результат
    this.handleTestResult(result);

    // Проверяем, завершен ли тест
    this.checkIfDone(result);
  };

  // Метод для обработки результатов теста от rmbtws
  handleTestResult(result) {
    this.testResult = result;
    
    console.log('=== TestVisualizationService.handleTestResult ===');
    console.log('Result:', result);
    console.log('Result status:', result?.status);
    console.log('Ping nano:', result?.pingNano);
    console.log('Down bit per sec:', result?.downBitPerSec);
    console.log('Up bit per sec:', result?.upBitPerSec);
    console.log('Ping median:', result?.pingMedian);
    console.log('All result properties:', Object.keys(result || {}));
    console.log('Full result object:', result);
    
    const status = result?.status?.toString();
    
    // Собираем данные пинга для графика (во всех фазах, где есть данные пинга)
    if (result && result.pingNano && result.pingNano !== -1 && result.pingNano !== undefined) {
      const currentTime = (Date.now() - this.startTime) / 1000; // время в секундах
      const pingMs = Math.round(result.pingNano / 1000000); // конвертируем в миллисекунды
      
      console.log('=== PING DATA PROCESSING ===');
      console.log('Result pingNano:', result.pingNano);
      console.log('Status:', status);
      console.log('Current phase:', this.currentPhase);
      console.log('Adding ping data:', { time: currentTime, ping: pingMs });
      
      // Добавляем в массив для вычисления медианы
      this.pingValues.push(pingMs);
      console.log('Total ping values collected:', this.pingValues.length);
      console.log('All ping values:', this.pingValues);
      
      this.pingData.push({
        time: currentTime,
        ping: pingMs
      });
      
      console.log('Total ping data points:', this.pingData.length);
      console.log('All ping data:', this.pingData);
      
      // Вызываем callback для обновления графика
      if (this.onPingUpdate) {
        console.log('Calling onPingUpdate with data:', this.pingData);
        this.onPingUpdate([...this.pingData]);
      } else {
        console.log('onPingUpdate callback is NOT set!');
      }
    } else {
      console.log('=== PING DATA SKIPPED ===');
      console.log('Result exists:', !!result);
      console.log('pingNano exists:', !!(result && result.pingNano));
      console.log('pingNano value:', result?.pingNano);
      console.log('pingNano !== -1:', result?.pingNano !== -1);
      console.log('pingNano !== undefined:', result?.pingNano !== undefined);
      console.log('Current status:', status);
      console.log('Current phase:', this.currentPhase);
    }
    
    // Обновляем текущую фазу и вычисляем медиану при смене фазы
    if (status && status !== this.currentPhase) {
      this.currentPhase = status;
      console.log('Phase changed to:', this.currentPhase);
      
      // Вычисляем медиану пинга при смене фазы
      if (this.pingValues.length > 0) {
        const median = this.calculateMedian(this.pingValues);
        console.log('=== PING MEDIAN CALCULATED ===');
        console.log('Ping values:', this.pingValues);
        console.log('Calculated median:', median);
        
        // Устанавливаем медиану
        this.pingMedian = median;
        console.log('Set pingMedian to:', this.pingMedian);
        
        if (this.onPingMedianUpdate) {
          console.log('Calling onPingMedianUpdate with calculated median:', median);
          this.onPingMedianUpdate(median);
        } else {
          console.log('onPingMedianUpdate callback is NOT set!');
        }
      }
      
      // Уведомляем о смене фазы
      if (this.onPhaseUpdate) {
        this.onPhaseUpdate(this.currentPhase, result);
      }
    }
    
    // Собираем данные download для графика (во время фазы DOWN)
    if (result && result.downBitPerSec && result.downBitPerSec !== -1 && status === 'DOWN') {
      const currentTime = (Date.now() - this.startTime) / 1000; // время в секундах
      const downloadMbps = (result.downBitPerSec / 1000000); // конвертируем в Mbps
      
      console.log('Adding download data:', { time: currentTime, speed: downloadMbps });
      
      this.downloadData.push({
        time: currentTime,
        speed: downloadMbps
      });
      
      console.log('Total download data points:', this.downloadData.length);
      
      // Вызываем callback для обновления графика
      if (this.onDownloadUpdate) {
        console.log('Calling onDownloadUpdate with data:', this.downloadData);
        this.onDownloadUpdate([...this.downloadData]);
      }
    }
    
    // Собираем данные upload для графика (во время фазы UP)
    if (result && result.upBitPerSec && result.upBitPerSec !== -1 && status === 'UP') {
      const currentTime = (Date.now() - this.startTime) / 1000; // время в секундах
      const uploadMbps = (result.upBitPerSec / 1000000); // конвертируем в Mbps
      
      console.log('Adding upload data:', { time: currentTime, speed: uploadMbps });
      
      this.uploadData.push({
        time: currentTime,
        speed: uploadMbps
      });
      
      console.log('Total upload data points:', this.uploadData.length);
      
      // Вызываем callback для обновления графика
      if (this.onUploadUpdate) {
        console.log('Calling onUploadUpdate with data:', this.uploadData);
        this.onUploadUpdate([...this.uploadData]);
      }
    }
    
    if (this.onProgress) {
      this.onProgress(result);
    }
  }

  // Проверяем, завершен ли тест
  checkIfDone(result) {
    const status = result?.status?.toString();
    
    switch (status) {
      case 'END':
        result.testUUID = this.testUUID;
        this.stopTest();
        if (this.onComplete) {
          this.onComplete(result);
        }
        break;
      case 'ERROR':
        this.stopTest();
        if (this.onError) {
          this.onError(result);
        }
        break;
      case 'ABORTED':
        this.stopTest();
        if (this.onError) {
          this.onError(result);
        }
        break;
      default:
        // Продолжаем цикл
        this.redrawLoop = setTimeout(this.draw, 160);
        break;
    }
  }

  // Методы для обработки событий
  setOnProgress(callback) {
    this.onProgress = callback;
  }

  setOnComplete(callback) {
    this.onComplete = callback;
  }

  setOnError(callback) {
    this.onError = callback;
  }

  // Метод для обновления графика пинга
  setOnPingUpdate(callback) {
    this.onPingUpdate = callback;
  }

  // Метод для обновления графика download
  setOnDownloadUpdate(callback) {
    this.onDownloadUpdate = callback;
  }

  // Метод для обновления графика upload
  setOnUploadUpdate(callback) {
    this.onUploadUpdate = callback;
  }

  // Метод для обновления фазы теста
  setOnPhaseUpdate(callback) {
    this.onPhaseUpdate = callback;
  }

  // Метод для обновления pingMedian
  setOnPingMedianUpdate(callback) {
    this.onPingMedianUpdate = callback;
  }

  // Метод для получения текущего результата
  getTestResult() {
    return this.testResult;
  }

  // Метод для получения данных пинга
  getPingData() {
    return this.pingData;
  }

  // Метод для получения данных download
  getDownloadData() {
    return this.downloadData;
  }

  // Метод для получения данных upload
  getUploadData() {
    return this.uploadData;
  }

  // Метод для получения текущей фазы
  getCurrentPhase() {
    return this.currentPhase;
  }

  // Метод для получения медианы пинга
  getPingMedian() {
    return this.pingMedian;
  }
}

export default TestVisualizationService; 