# Deploy to GitHub Pages

## Автоматический деплой

Приложение автоматически деплоится на GitHub Pages при каждом push в ветку `main`.

### URL приложения
- **Production**: https://specure.github.io/nettest
- **Development**: http://localhost:3000

## Ручной деплой

Если нужно задеплоить вручную:

```bash
# Установить зависимости
cd static
npm install

# Собрать приложение
npm run build

# Задеплоить на GitHub Pages
npm run deploy
```

## Настройки GitHub Pages

1. Перейти в Settings репозитория
2. Найти раздел "Pages"
3. Source: "Deploy from a branch"
4. Branch: "gh-pages"
5. Folder: "/ (root)"

## Структура файлов

```
├── .github/workflows/deploy.yml  # GitHub Actions
├── static/
│   ├── package.json              # Настройки для gh-pages
│   ├── public/
│   │   └── _redirects           # Для React Router
│   └── src/                      # Исходный код
└── DEPLOY.md                     # Эта инструкция
```

## Проверка деплоя

После деплоя проверить:
1. https://specure.github.io/nettest - загружается ли приложение
2. Карта серверов - отображаются ли серверы
3. Измерения - работают ли тесты
4. Документация - открывается ли модальное окно

## Troubleshooting

### Проблема: 404 на GitHub Pages
**Решение**: Проверить файл `_redirects` в `static/public/`

### Проблема: CORS ошибки
**Решение**: Убедиться что API endpoints настроены правильно

### Проблема: Карта не загружается
**Решение**: Проверить что Leaflet загружается корректно 