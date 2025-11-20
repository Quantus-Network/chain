FROM grafana/grafana:11.3.0

USER root

# Copy Quantus branding assets
COPY branding/logo.svg /usr/share/grafana/public/img/grafana_icon.svg
COPY branding/favicon.ico /usr/share/grafana/public/img/favicon.ico
COPY branding/favicon.ico /usr/share/grafana/public/img/fav32.png
COPY branding/logo.png /usr/share/grafana/public/img/apple-touch-icon.png

USER grafana

