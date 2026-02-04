{{/*
Expand the name of the chart.
*/}}
{{- define "shadowmesh.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "shadowmesh.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "shadowmesh.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "shadowmesh.labels" -}}
helm.sh/chart: {{ include "shadowmesh.chart" . }}
{{ include "shadowmesh.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "shadowmesh.selectorLabels" -}}
app.kubernetes.io/name: {{ include "shadowmesh.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Gateway labels
*/}}
{{- define "shadowmesh.gateway.labels" -}}
{{ include "shadowmesh.labels" . }}
app.kubernetes.io/component: gateway
{{- end }}

{{/*
Gateway selector labels
*/}}
{{- define "shadowmesh.gateway.selectorLabels" -}}
{{ include "shadowmesh.selectorLabels" . }}
app.kubernetes.io/component: gateway
{{- end }}

{{/*
Node labels
*/}}
{{- define "shadowmesh.node.labels" -}}
{{ include "shadowmesh.labels" . }}
app.kubernetes.io/component: node
{{- end }}

{{/*
Node selector labels
*/}}
{{- define "shadowmesh.node.selectorLabels" -}}
{{ include "shadowmesh.selectorLabels" . }}
app.kubernetes.io/component: node
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "shadowmesh.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "shadowmesh.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Gateway image
*/}}
{{- define "shadowmesh.gateway.image" -}}
{{- $registry := .Values.global.imageRegistry | default "" -}}
{{- $repository := .Values.gateway.image.repository -}}
{{- $tag := .Values.gateway.image.tag | default .Chart.AppVersion -}}
{{- if $registry -}}
{{- printf "%s/%s:%s" $registry $repository $tag -}}
{{- else -}}
{{- printf "%s:%s" $repository $tag -}}
{{- end -}}
{{- end }}

{{/*
Node image
*/}}
{{- define "shadowmesh.node.image" -}}
{{- $registry := .Values.global.imageRegistry | default "" -}}
{{- $repository := .Values.node.image.repository -}}
{{- $tag := .Values.node.image.tag | default .Chart.AppVersion -}}
{{- if $registry -}}
{{- printf "%s/%s:%s" $registry $repository $tag -}}
{{- else -}}
{{- printf "%s:%s" $repository $tag -}}
{{- end -}}
{{- end }}
