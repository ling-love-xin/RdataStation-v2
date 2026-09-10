<template>
  <div class="net-tab">
    <!-- File DB: no network needed -->
    <div v-if="driver?.is_file" class="net-file-hint">
      <Database :size="36" class="hint-icon" />
      <h3>{{ $t('connection.networkTab.fileDbHint') }}</h3>
      <p>{{ $t('connection.networkTab.fileDbHintDesc') }}</p>
    </div>

    <!-- Empty driver -->
    <div v-else-if="!driver" class="empty-hint">{{ $t('navigator.noDriver') }}</div>

    <!-- Network config -->
    <template v-else>
      <div class="net-hint">
        <strong>{{ $t('connection.networkTab.dynamicChain') }}</strong>
        {{ $t('connection.networkTab.chainDesc') }}
      </div>

      <!-- Chain header -->
      <div class="chain-header">
        <span class="ch-drag" />
        <span class="ch-order">#</span>
        <span class="ch-type">{{ $t('connection.networkTab.protocol') }}</span>
        <span class="ch-config">{{ $t('connection.networkTab.configuration') }}</span>
        <span class="ch-toggle">{{ $t('connection.networkTab.enable') }}</span>
        <span class="ch-acts">{{ $t('connection.networkTab.actions') }}</span>
      </div>

      <!-- Chain list -->
      <div class="chain-list">
        <div
          v-for="(hop, idx) in filteredChain"
          :key="hop.id"
          :class="hopItemClass(hop)"
          :draggable="true"
          @dragstart="dragStart($event, hop.id)"
          @dragover.prevent="dragOver($event)"
          @dragleave="dragLeave($event)"
          @drop="drop($event, hop.id)"
          @dragend="dragEnd"
        >
          <div
            class="drag-handle"
            :title="
              hop.protocol === 'ssl'
                ? $t('connection.networkTab.sslLocked')
                : $t('connection.networkTab.dragHint')
            "
          >
            {{ hop.protocol === 'ssl' ? '🔒' : '≡' }}
          </div>
          <span :class="['order-badge', hop.protocol]">{{ orderNum(idx, hop) }}</span>
          <span :class="['type-badge', hop.protocol]"
            >{{ hopIcon(hop.protocol) }} {{ hopLabel(hop.protocol) }}</span
          >

          <div class="config-area">
            <!-- Select mode: dropdown + actions -->
            <div v-if="hop.mode === 'select'" class="select-row">
              <NSelect
                v-model:value="hop.profileId"
                size="small"
                :options="profileOpts(hop.protocol)"
                :placeholder="selectPlaceholder(hop.protocol)"
                class="config-select"
                filterable
              />
              <NButton size="tiny" type="primary" ghost @click="setHopMode(hop.id, 'new')"
                >+ {{ t('navigator.newConfig') }}</NButton
              >
              <NButton
                size="tiny"
                quaternary
                class="custom-link"
                @click="setHopMode(hop.id, 'custom')"
                >{{ t('navigator.customConfig') }}</NButton
              >
            </div>

            <!-- New mode: inline form -->
            <div v-else-if="hop.mode === 'new'" class="inline-form-v5">
              <!-- Name + Scope -->
              <div class="form-row">
                <div class="form-group f1">
                  <span class="form-label">{{ t('navigator.configName') }}</span>
                  <NInput
                    v-model:value="newFormData[hop.id].name"
                    size="small"
                    :placeholder="t('navigator.newSshProfile')"
                  />
                </div>
                <div class="form-group f1">
                  <span class="form-label">{{ t('connection.networkTab.formScope') }}</span>
                  <span class="profile-scope-badge">{{ profScopeLabel() }}</span>
                </div>
              </div>

              <template v-if="hop.protocol === 'ssh'">
                <!-- Section: Bastion Connection -->
                <div class="form-section-label">🔗 {{ t('navigator.sectionBastion') }}</div>
                <div class="form-row">
                  <div class="form-group f2">
                    <span class="form-label">{{ t('navigator.host') }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].host"
                      size="small"
                      placeholder="192.168.1.1"
                    />
                  </div>
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.port') }}</span>
                    <NInputNumber
                      v-model:value="newFormData[hop.id].port"
                      size="small"
                      :min="1"
                      :max="65535"
                      :default-value="22"
                    />
                  </div>
                </div>
                <!-- Section: SSH Auth -->
                <div class="form-section-label"
                  >🔐 {{ t('navigator.sshAuthTitle') || 'SSH 认证' }}</div
                >
                <div class="form-row">
                  <div class="form-group f1">
                    <NSelect
                      v-model:value="newFormData[hop.id].authType"
                      size="small"
                      :options="sshAuthTypeOpts"
                      :placeholder="t('navigator.authMethod')"
                      @update:value="() => onChainAuthChange(hop.id, newFormData[hop.id].authType)"
                    />
                  </div>
                  <div class="form-group" style="flex: 1.6">
                    <NSelect
                      v-model:value="newFormData[hop.id].savedAuthId"
                      size="small"
                      :options="chainAuthCfgOpts"
                      :placeholder="t('navigator.manualFill') || '— 手动填写 —'"
                      clearable
                      filterable
                    />
                  </div>
                </div>
                <div v-if="newFormData[hop.id].authType === 'ssh_password'" class="form-row">
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.sshUser') || 'SSH 用户' }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].username"
                      size="small"
                      :placeholder="'root'"
                    />
                  </div>
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.sshPassword') || 'SSH 密码' }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].password"
                      size="small"
                      type="password"
                      :placeholder="t('navigator.password')"
                    />
                  </div>
                </div>
                <template v-else>
                  <div class="form-row">
                    <div class="form-group f1">
                      <span class="form-label">{{ t('navigator.sshUser') || 'SSH 用户' }}</span>
                      <NInput
                        v-model:value="newFormData[hop.id].username"
                        size="small"
                        :placeholder="'root'"
                      />
                    </div>
                    <div class="form-group f1">
                      <span class="form-label">Passphrase</span>
                      <NInput
                        v-model:value="newFormData[hop.id].passphrase"
                        size="small"
                        type="password"
                        :placeholder="t('navigator.privateKeyPassphrase') || '私钥密码（可选）'"
                      />
                    </div>
                  </div>
                  <div class="form-row">
                    <div class="form-group" style="flex: 1">
                      <span class="form-label">{{
                        t('navigator.privateKeyFile') || '私钥文件'
                      }}</span>
                      <div class="file-input-row">
                        <NInput
                          v-model:value="newFormData[hop.id].keyPath"
                          size="small"
                          :placeholder="'~/.ssh/id_rsa'"
                        />
                        <NButton size="small" quaternary>📂</NButton>
                      </div>
                    </div>
                  </div>
                </template>
                <div class="form-row">
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.keepAliveInterval') }}</span>
                    <NInputNumber
                      v-model:value="newFormData[hop.id].keepAlive"
                      size="small"
                      :min="0"
                      :max="600"
                      :default-value="60"
                    />
                  </div>
                </div>
                <!-- Section: Port Forwarding -->
                <div class="form-section-label">📡 {{ t('navigator.sectionPortForward') }}</div>
                <div class="form-row">
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.localPort') }}</span>
                    <NInputNumber
                      v-model:value="newFormData[hop.id].localPort"
                      size="small"
                      :min="1"
                      :max="65535"
                      :placeholder="t('navigator.autoAssign')"
                    />
                  </div>
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.remoteHost') }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].remoteHost"
                      size="small"
                      :placeholder="t('navigator.targetDbHost')"
                    />
                  </div>
                  <div class="form-group" style="width: 80px">
                    <span class="form-label">{{ t('navigator.port') }}</span>
                    <NInputNumber
                      v-model:value="newFormData[hop.id].remotePort"
                      size="small"
                      :min="1"
                      :max="65535"
                      placeholder="3306"
                    />
                  </div>
                </div>
                <div class="form-hint">{{ t('navigator.sectionPortForwardHint') }}</div>
              </template>
              <template v-else-if="hop.protocol === 'ssl'">
                <div class="form-row">
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.sslMode') }}</span>
                    <NSelect
                      v-model:value="newFormData[hop.id].sslMode"
                      size="small"
                      :options="sslModeOpts"
                      :placeholder="t('navigator.sslMode')"
                    />
                  </div>
                </div>
                <div class="form-row">
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.caFilePath') }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].ca"
                      size="small"
                      placeholder="ca.pem"
                    />
                  </div>
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.certFilePath') }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].cert"
                      size="small"
                      placeholder="client.pem"
                    />
                  </div>
                </div>
                <div class="form-row">
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.keyFilePath') }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].key"
                      size="small"
                      :placeholder="t('navigator.keyFilePath')"
                    />
                  </div>
                </div>
              </template>
              <template v-else-if="hop.protocol === 'proxy'">
                <div class="form-row">
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.proxyType') }}</span>
                    <NSelect
                      v-model:value="newFormData[hop.id].proxyType"
                      size="small"
                      :options="proxyTypeOpts"
                      :placeholder="t('navigator.proxyType')"
                    />
                  </div>
                </div>
                <div class="form-row">
                  <div class="form-group f2">
                    <span class="form-label">{{ t('navigator.host') }}</span>
                    <NInput
                      v-model:value="newFormData[hop.id].host"
                      size="small"
                      placeholder="proxy.example.com"
                    />
                  </div>
                  <div class="form-group f1">
                    <span class="form-label">{{ t('navigator.port') }}</span>
                    <NInputNumber
                      v-model:value="newFormData[hop.id].port"
                      size="small"
                      :min="1"
                      :max="65535"
                      :default-value="1080"
                    />
                  </div>
                </div>
                <!-- Section: Proxy Auth -->
                <div class="form-section-label"
                  >🔐 {{ t('navigator.proxyAuthTitle') || '代理认证（可选）' }}</div
                >
                <div class="form-row">
                  <div class="form-group f1">
                    <NSelect
                      v-model:value="newFormData[hop.id].authType"
                      size="small"
                      :options="proxyAuthTypeOpts"
                      :placeholder="t('navigator.authMethod')"
                    />
                  </div>
                  <div class="form-group" style="flex: 1.6">
                    <NSelect
                      v-model:value="newFormData[hop.id].savedAuthId"
                      size="small"
                      :options="chainAuthCfgOpts"
                      :placeholder="t('navigator.manualFill') || '— 手动填写 —'"
                      clearable
                      filterable
                    />
                  </div>
                </div>
                <template v-if="newFormData[hop.id].authType === 'password'">
                  <div class="form-row">
                    <div class="form-group f1">
                      <span class="form-label">{{ t('navigator.username') }}</span>
                      <NInput
                        v-model:value="newFormData[hop.id].username"
                        size="small"
                        :placeholder="t('navigator.usernameOptional')"
                      />
                    </div>
                    <div class="form-group f1">
                      <span class="form-label">{{ t('navigator.password') }}</span>
                      <NInput
                        v-model:value="newFormData[hop.id].password"
                        size="small"
                        type="password"
                        :placeholder="t('navigator.passwordOptional')"
                      />
                    </div>
                  </div>
                </template>
              </template>
              <div class="new-save-row">
                <NButton
                  size="tiny"
                  type="primary"
                  :loading="creating[hop.id]"
                  @click="saveNewProfile(hop)"
                  >{{ t('navigator.saveApply') || '保存并应用' }}</NButton
                >
                <NButton
                  size="tiny"
                  secondary
                  :loading="testingHop[hop.id]"
                  class="btn-test-conn"
                  @click="testChainHop(hop)"
                >
                  🧪 {{ t('navigator.testConnection') }}
                </NButton>
                <NButton size="tiny" quaternary @click="setHopMode(hop.id, 'select')">{{
                  t('navigator.cancel')
                }}</NButton>
              </div>
            </div>

            <!-- Custom mode: one-time form -->
            <div v-else-if="hop.mode === 'custom'" class="inline-form-v5 custom">
              <div class="custom-hint"
                >⚡ {{ t('navigator.customOneTime') || '一次性自定义 — 不保存为配置文件' }}</div
              >

              <!-- SSH custom fields -->
              <template v-if="hop.protocol === 'ssh'">
                <div class="form-section-label"
                  >🔗 {{ t('navigator.jumpHost') || '跳板机连接' }}</div
                >
                <div class="form-row">
                  <div class="form-group f2"
                    >{{ t('navigator.host') }}
                    <NInput
                      v-model:value="customData[hop.id].host"
                      size="small"
                      :placeholder="t('navigator.hostPlaceholder')"
                  /></div>
                  <div class="form-group f1"
                    >{{ t('navigator.port') }}
                    <NInputNumber
                      v-model:value="customData[hop.id].port"
                      size="small"
                      :min="1"
                      :max="65535"
                  /></div>
                </div>
                <div class="form-row">
                  <div class="form-group f1"
                    >{{ t('navigator.username') }}
                    <NInput
                      v-model:value="customData[hop.id].username"
                      size="small"
                      placeholder="root"
                  /></div>
                  <div class="form-group f1"
                    >{{ t('navigator.authMethod') }}
                    <NSelect
                      v-model:value="customData[hop.id].authType"
                      size="small"
                      :options="sshAuthTypeOpts"
                  /></div>
                </div>
                <div v-if="customData[hop.id].authType === 'ssh_password'" class="form-row">
                  <div class="form-group f2"
                    >{{ t('navigator.password') }}
                    <NInput
                      v-model:value="customData[hop.id].password"
                      type="password"
                      size="small"
                  /></div>
                </div>
                <template v-else>
                  <div class="form-row">
                    <div class="form-group f2"
                      >{{ t('navigator.keyPath') || '私钥路径' }}
                      <NInput
                        v-model:value="customData[hop.id].keyPath"
                        size="small"
                        placeholder="~/.ssh/id_rsa"
                    /></div>
                  </div>
                  <div class="form-row">
                    <div class="form-group f1"
                      >{{ t('navigator.passphrase') || '密钥密码' }}
                      <NInput
                        v-model:value="customData[hop.id].passphrase"
                        type="password"
                        size="small"
                    /></div>
                  </div>
                </template>
              </template>

              <!-- SSL custom fields -->
              <template v-if="hop.protocol === 'ssl'">
                <div class="form-row">
                  <div class="form-group f1"
                    >{{ t('navigator.sslMode') }}
                    <NSelect
                      v-model:value="customData[hop.id].sslMode"
                      size="small"
                      :options="sslModeOpts"
                  /></div>
                </div>
                <div class="form-row">
                  <div class="form-group f2"
                    >{{ t('navigator.caCert') || 'CA 证书' }}
                    <NInput
                      v-model:value="customData[hop.id].ca"
                      size="small"
                      placeholder="/path/to/ca.pem"
                  /></div>
                </div>
                <div class="form-row">
                  <div class="form-group f2"
                    >{{ t('navigator.clientCert') || '客户端证书' }}
                    <NInput v-model:value="customData[hop.id].cert" size="small"
                  /></div>
                </div>
                <div class="form-row">
                  <div class="form-group f2"
                    >{{ t('navigator.clientKey') || '客户端密钥' }}
                    <NInput v-model:value="customData[hop.id].key" size="small"
                  /></div>
                </div>
              </template>

              <!-- Proxy custom fields -->
              <template v-if="hop.protocol === 'proxy'">
                <div class="form-row">
                  <div class="form-group f1"
                    >{{ t('navigator.proxyType') }}
                    <NSelect
                      v-model:value="customData[hop.id].proxyType"
                      size="small"
                      :options="proxyTypeOpts"
                  /></div>
                </div>
                <div class="form-row">
                  <div class="form-group f2"
                    >{{ t('navigator.host') }}
                    <NInput v-model:value="customData[hop.id].host" size="small"
                  /></div>
                  <div class="form-group f1"
                    >{{ t('navigator.port') }}
                    <NInputNumber
                      v-model:value="customData[hop.id].port"
                      size="small"
                      :min="1"
                      :max="65535"
                  /></div>
                </div>
                <div class="form-row">
                  <div class="form-group f1"
                    >{{ t('navigator.username') }}
                    <NInput v-model:value="customData[hop.id].username" size="small"
                  /></div>
                  <div class="form-group f1"
                    >{{ t('navigator.password') }}
                    <NInput
                      v-model:value="customData[hop.id].password"
                      type="password"
                      size="small"
                  /></div>
                </div>
              </template>

              <div class="form-row" style="justify-content: flex-end; gap: 8px">
                <NButton
                  size="tiny"
                  quaternary
                  type="warning"
                  @click="setHopMode(hop.id, 'select')"
                  >{{ t('navigator.closeCustom') || '关闭自定义' }}</NButton
                >
              </div>
            </div>

            <!-- SSH forward info -->
            <div
              v-if="hop.protocol === 'ssh' && hop.mode === 'select' && hop.profileId"
              class="forward-info"
            >
              <span v-if="getForwardInfo(hop.profileId)" class="forward-tag">{{
                getForwardInfo(hop.profileId)
              }}</span>
            </div>
          </div>

          <div class="toggle-wrap">
            <NSwitch v-model:value="hop.enabled" size="small" />
          </div>

          <div class="act-wrap">
            <NButton
              text
              size="tiny"
              :title="t('navigator.networkProfileManager')"
              @click="openProfileMgr(hop)"
              >📋</NButton
            >
            <NButton
              v-if="canDelete(hop)"
              text
              size="tiny"
              class="del-btn"
              :title="t('navigator.deleteNode')"
              @click="deleteHopWrapped(hop.id)"
              >✕</NButton
            >
            <span v-else class="no-del">✕</span>
          </div>
        </div>
        <div v-if="chain.length === 0" class="net-empty"
          >🔗 {{ $t('connection.networkTab.directConnect') }}</div
        >
      </div>

      <!-- Warning with latency estimate -->
      <div v-if="enabledHopCount >= 3" class="net-warning">
        ⚠️
        {{
          t('navigator.latencyWarning', { count: enabledHopCount, latency: enabledHopCount * 25 })
        }}
      </div>

      <!-- Add hop -->
      <div class="add-hop-row">
        <NButton v-if="canAddSshProxy" size="small" dashed @click="menuOpen = !menuOpen">{{
          addHopButtonLabel()
        }}</NButton>
        <NButton v-else-if="canAddSsl" size="small" dashed @click="addHopWrapped('ssl')">{{
          $t('connection.networkTab.addTls')
        }}</NButton>
        <span v-else class="hop-limit">{{ $t('connection.networkTab.chainFull') }}</span>

        <div v-if="menuOpen && canAddSshProxy" class="hop-menu">
          <div
            v-if="canAddSsh"
            class="hop-opt"
            @click="addHopWrapped('ssh')"
            >🔒 SSH {{ t('navigator.remainingHops', { n: maxHopsRemaining() }) }}</div
          >
          <div
            v-if="canAddProxy"
            class="hop-opt"
            @click="addHopWrapped('proxy')"
            >🌐 Proxy {{ t('navigator.remainingHops', { n: maxHopsRemaining() }) }}</div
          >
          <div
            v-if="canAddSsl"
            class="hop-opt"
            @click="addHopWrapped('ssl')"
            >{{ sslMenuLabel() }}</div
          >
          <div class="hop-menu-sep"></div>
          <div class="hop-menu-desc">🛡 {{ t('navigator.sslTailHint') }}</div>
        </div>
      </div>

      <!-- Topology preview -->
      <TopologyPreview :hops="topoHops" :db-label="dbLabel" />
    </template>

    <!-- ========== Profile Manager (NetworkConfigManager) ========== -->
    <NetworkConfigManager
      :visible="showProfileMgr"
      :scope="props.scope"
      :default-tab="profileMgrTab"
      :ssh-profiles="rawSshProfiles"
      :ssl-profiles="rawSslProfiles"
      :proxy-profiles="rawProxyProfiles"
      @close="showProfileMgr = false"
      @create-ssh="handleCreateSshProfile"
      @create-ssl="handleCreateSslProfile"
      @create-proxy="handleCreateProxyProfile"
      @delete-ssh="handleDeleteSshProfile"
      @delete-ssl="handleDeleteSslProfile"
      @delete-proxy="handleDeleteProxyProfile"
    />
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { Database } from 'lucide-vue-next'
import { NButton, NInput, NInputNumber, NSelect, NSwitch, useMessage } from 'naive-ui'
import { ref, computed, onMounted, watch, reactive } from 'vue'
import { useI18n } from 'vue-i18n'

import { useNetworkChain } from '../../composables/useNetworkChain'
import { useNetworkProfileBridge } from '../../composables/useNetworkProfileBridge'
import { useNetworkProfiles } from '../../composables/useNetworkProfiles'
import NetworkConfigManager from '../network/NetworkConfigManager.vue'
import TopologyPreview from '../network/TopologyPreview.vue'

import type { Driver } from '../../../domain/types'
import type { AuthConfig } from '../../composables/useAuthConfig'
import type { NetworkProfile } from '../../composables/useNetworkProfiles'
import type { ProtocolType, ProtocolNode, HopConfigMode } from '../../types/network-chain'
import type { TopoHop } from '../network/TopologyPreview.vue'

const props = defineProps<{
  driver?: Driver | null
  scope?: { global: boolean; project: boolean }
}>()

const emit = defineEmits<{
  'extra-config': [config: Record<string, unknown>]
}>()

const {
  sshProfiles,
  sslProfiles,
  proxyProfiles,
  loadAll,
  loadAllProject,
  saveProjectProfile,
  removeProjectProfile,
  getProjectPath,
} = useNetworkProfiles()
const { t } = useI18n()
const message = useMessage()

// ===== Network profile bridge (SSH/SSL/Proxy CRUD) =====
const {
  createSshProfile: handleCreateSshProfile,
  createSslProfile: handleCreateSslProfile,
  createProxyProfile: handleCreateProxyProfile,
  deleteSshProfile: handleDeleteSshProfile,
  deleteSslProfile: handleDeleteSslProfile,
  deleteProxyProfile: handleDeleteProxyProfile,
} = useNetworkProfileBridge({
  isProject: !!props.scope?.project,
  getProjectPath,
  saveProjectProfile,
  loadAllProject,
  loadAll,
  removeProjectProfile,
})

// ==================== Chain Model (via composable) ====================

// Canonical type alias — ProtocolNode from network-chain.ts
type Hop = ProtocolNode

const {
  chain,
  menuOpen,
  enabledNetworkHopCount: _enabledNetworkHopCount,
  hasSsl,
  isMaxNetworkHops,
  remainingHops,
  countInstancesOfType,
  addHop: chainAddHop,
  deleteHop: chainDeleteHop,
  switchHopMode: composableSwitchHopMode,
  onDragStart: composableDragStart,
  onDragEnd: composableDragEnd,
  onDrop: composableDrop,
} = useNetworkChain([
  {
    id: 'h1',
    protocol: 'ssh' as ProtocolType,
    enabled: false,
    mode: 'select' as HopConfigMode,
    profileId: '',
  },
  {
    id: 'h2',
    protocol: 'proxy' as ProtocolType,
    enabled: false,
    mode: 'select' as HopConfigMode,
    profileId: '',
  },
  {
    id: 'h3',
    protocol: 'ssl' as ProtocolType,
    enabled: false,
    mode: 'select' as HopConfigMode,
    profileId: '',
  },
])

/** Chain items filtered by driver capabilities */
const filteredChain = computed(() => chain.value.filter(hop => shouldShowHop(hop.protocol)))

// ===== Wrappers for template compatibility =====
function addHopWrapped(protocol: ProtocolType) {
  const newId = chainAddHop(protocol)
  if (newId) ensureForm(newId)
  menuOpen.value = false
}
function deleteHopWrapped(id: string) {
  chainDeleteHop(id)
}
function setHopMode(id: string, mode: HopConfigMode) {
  composableSwitchHopMode(id, mode)
  ensureForm(id)
}
function maxHopsRemaining(): number {
  return remainingHops.value
}

// ==================== Inline New/Custom Forms ====================

interface NewFormFields {
  name: string
  host: string
  port: number | null
  username: string
  authType: string
  password: string
  keyPath: string
  passphrase: string
  savedAuthId: string
  localPort: number | null
  remoteHost: string
  remotePort: number | null
  keepAlive: number | null
  sslMode: string
  ca: string
  cert: string
  key: string
  proxyType: string
}

function blankNewForm(): NewFormFields {
  return {
    name: '',
    host: '',
    port: null,
    username: '',
    authType: 'ssh_password',
    password: '',
    keyPath: '',
    passphrase: '',
    savedAuthId: '',
    localPort: null,
    remoteHost: '',
    remotePort: null,
    keepAlive: 60,
    sslMode: 'verify-full',
    ca: '',
    cert: '',
    key: '',
    proxyType: 'socks5',
  }
}

const newFormData = reactive<Record<string, NewFormFields>>({})
const customData = reactive<Record<string, NewFormFields>>({})
const creating = reactive<Record<string, boolean>>({})

function ensureForm(hopId: string) {
  if (!newFormData[hopId]) newFormData[hopId] = blankNewForm()
  if (!customData[hopId]) customData[hopId] = blankNewForm()
  if (creating[hopId] === undefined) creating[hopId] = false
}

// ==================== Active Hop for Profile Manager ====================

const activeProfileMgrHop = ref<Hop | null>(null)

// ==================== Driver Capabilities (基于 capabilities 字段动态控制 UI) ====================

/** 网络协议关键词（用于判定 capabilities 是否显式声明了网络能力） */
const NETWORK_CAP_KEYS = ['ssh', 'ssh_tunnel', 'ssl', 'ssl_tls', 'proxy'] as const

/** 解析驱动的 capabilities 字段，返回支持的协议列表 */
function parseDriverCapabilities(capabilities: string | undefined): string[] {
  if (!capabilities) return []
  try {
    return JSON.parse(capabilities) as string[]
  } catch {
    return []
  }
}

/**
 * 检查指定网络能力是否受支持
 *
 * 规则：
 * 1. capabilities 为空 → 默认启用所有网络协议（向后兼容未设置 capabilities 的驱动）
 * 2. capabilities 不含任何网络关键词 → 默认启用所有网络协议（capabilities 仅声明了数据库功能，网络能力不受限）
 * 3. capabilities 含网络关键词 → 按显式声明判断
 */
function hasNetworkCap(caps: string[], targetKeys: readonly string[]): boolean {
  if (caps.length === 0) return true
  const hasExplicitNetworkCaps = caps.some(c => (NETWORK_CAP_KEYS as readonly string[]).includes(c))
  if (!hasExplicitNetworkCaps) return true
  return caps.some(c => targetKeys.includes(c))
}

/** 是否支持 SSL/TLS */
const supportsSsl = computed(() => {
  const caps = parseDriverCapabilities(props.driver?.capabilities)
  return hasNetworkCap(caps, ['ssl', 'ssl_tls'])
})

/** 是否支持 SSH 隧道 */
const supportsSsh = computed(() => {
  const caps = parseDriverCapabilities(props.driver?.capabilities)
  return hasNetworkCap(caps, ['ssh', 'ssh_tunnel'])
})

/** 是否支持代理 */
const supportsProxy = computed(() => {
  const caps = parseDriverCapabilities(props.driver?.capabilities)
  return hasNetworkCap(caps, ['proxy'])
})

// ==================== Computed ====================

const enabledHopCount = computed(() => _enabledNetworkHopCount.value)
const sslInChain = computed(() => hasSsl.value)
const canAddSshProxy = computed(
  () => !isMaxNetworkHops.value && (supportsSsh.value || supportsProxy.value)
)
const dbLabel = computed(() => props.driver?.name?.toUpperCase() || 'DB')

/** 是否可以添加 SSL（驱动支持且链中还没有 SSL） */
const canAddSsl = computed(() => supportsSsl.value && !sslInChain.value)

/** 是否可以添加 SSH（驱动支持且未达上限） */
const canAddSsh = computed(() => supportsSsh.value && remainingHops.value > 0)

/** 是否可以添加 Proxy（驱动支持且未达上限） */
const canAddProxy = computed(() => supportsProxy.value && remainingHops.value > 0)

/** Map chain hops to TopoHop format for the topology preview */
const topoHops = computed<TopoHop[]>(() =>
  chain.value.map(h => ({
    id: h.id,
    protocol: h.protocol,
    enabled: h.enabled,
    mode: h.mode,
    profileId: h.profileId,
    host: newFormData[h.id]?.host || undefined,
    port: (newFormData[h.id]?.port ?? undefined) as number | undefined,
  }))
)

// Direct access to raw profiles (unwrap ComputedRef)
const rawSshProfiles = computed(() => sshProfiles.value)
const rawSslProfiles = computed(() => sslProfiles.value)
const rawProxyProfiles = computed(() => proxyProfiles.value)

const sslModeOpts = [
  { label: 'verify-full', value: 'verify-full' },
  { label: 'verify-ca', value: 'verify-ca' },
  { label: 'require', value: 'require' },
]

const proxyTypeOpts = [
  { label: 'SOCKS5', value: 'socks5' },
  { label: 'HTTP', value: 'http' },
  { label: 'SOCKS4', value: 'socks4' },
]

const proxyAuthTypeOpts = [
  { label: t('navigator.noAuth') || '— 无认证 —', value: '' },
  { label: '🔑 ' + (t('navigator.usernamePassword') || '用户名/密码'), value: 'password' },
]

function profScopeLabel(): string {
  return props.scope?.global ? `🌐 ${t('navigator.global')}` : `📝 ${t('navigator.project')}`
}

// ==================== Profile Options ====================

function selectPlaceholder(p: string): string {
  return (
    {
      ssh: t('navigator.selectSsh'),
      ssl: t('navigator.selectSsl'),
      proxy: t('navigator.selectProxy'),
    }[p] || t('navigator.selectProfile')
  )
}

function profileOpts(p: string) {
  const profiles = { ssh: rawSshProfiles, ssl: rawSslProfiles, proxy: rawProxyProfiles }[p]
  return (profiles?.value ?? []).map(x => {
    const scopeIcon = x.origin === 'global' ? '🌐' : '📝'
    return { label: `${scopeIcon} ${x.name} (${x.detail})`, value: x.id }
  })
}

function findProfile(type: string, id: string): NetworkProfile | undefined {
  const list =
    { ssh: rawSshProfiles.value, ssl: rawSslProfiles.value, proxy: rawProxyProfiles.value }[type] ??
    []
  return list.find(p => p.id === id)
}

// ==================== Hop Helpers ====================

/** 是否应该显示某个协议的节点（基于驱动 capabilities） */
function shouldShowHop(protocol: ProtocolType): boolean {
  if (!props.driver) return false
  switch (protocol) {
    case 'ssl':
      return supportsSsl.value
    case 'ssh':
      return supportsSsh.value
    case 'proxy':
      return supportsProxy.value
    default:
      return true
  }
}

function hopItemClass(hop: Hop) {
  const base = hop.protocol === 'ssl' ? 'chain-item ssl' : 'chain-item'
  return base + (hop.enabled ? '' : ' disabled')
}

function orderNum(idx: number, hop: Hop) {
  if (hop.protocol === 'ssl') return hop.enabled ? '🔐' : '-'
  const netHops = chain.value.filter(h => h.protocol !== 'ssl' && h.enabled)
  return hop.enabled ? String(netHops.indexOf(hop) + 1) : '-'
}

function hopIcon(p: string) {
  return { ssh: '🔒', ssl: '🛡', proxy: '🌐' }[p] || ''
}
function hopLabel(p: string) {
  return { ssh: 'SSH 隧道', ssl: 'SSL/TLS (末尾)', proxy: '代理' }[p] || p
}

function canDelete(hop: Hop) {
  return countInstancesOfType(hop.protocol) > 1
}

// SSH forward info
function getForwardInfo(profileId: string): string {
  const pf = findProfile('ssh', profileId)
  if (!pf) return ''
  const cfg = pf.config as Record<string, unknown> | null
  if (!cfg) return ''
  const remoteHost = cfg.remoteHost || 'DB'
  const remotePort = cfg.remotePort || 'auto'
  const localPort = cfg.localPort || 'auto'
  return `📡 转发: ${remoteHost}:${remotePort} → 127.0.0.1:${localPort}`
}

// ==================== Add Hop Button & Menu ====================

function addHopButtonLabel(): string {
  return t('connection.networkTab.addHop')
}

function sslMenuLabel(): string {
  if (sslInChain.value) {
    const existingSsl = chain.value.find(h => h.protocol === 'ssl')
    return existingSsl && existingSsl.enabled ? '🛡 SSL/TLS (替换)' : '🛡 SSL/TLS'
  }
  return '🛡 SSL/TLS'
}

// ==================== Save New Profile (inline) ====================

async function saveNewProfile(hop: Hop) {
  const id = hop.id
  if (!id || creating[id]) return
  creating[id] = true
  try {
    const f = newFormData[id]
    const isProject = !!props.scope?.project
    const projectPath = isProject ? await getProjectPath() : null

    // SSH/Proxy 有认证凭据时，同步创建 auth_config 并注入引用
    let savedAuthConfigId: string | null = null
    if (
      (hop.protocol === 'ssh' || hop.protocol === 'proxy') &&
      (f.username ||
        f.password ||
        (hop.protocol === 'ssh' && f.authType !== 'ssh_password' && f.keyPath))
    ) {
      try {
        const authData: Record<string, unknown> = {}
        if (f.username) authData.username = f.username
        if (f.password) authData.password = f.password
        if (hop.protocol === 'ssh') {
          authData.authType = f.authType || 'ssh_password'
          if (f.authType !== 'ssh_password' && f.keyPath) authData.keyPath = f.keyPath
          if (f.passphrase) authData.passphrase = f.passphrase
        }
        const authName = `${f.name || '未命名'} — ${hop.protocol}认证`
        const authType = hop.protocol === 'ssh' ? f.authType || 'ssh_password' : 'proxy_password'

        if (isProject && projectPath) {
          const created = await invoke<{ id: string }>('project_create_auth_config', {
            name: authName,
            authType,
            authData: JSON.stringify(authData),
            projectPath,
          })
          savedAuthConfigId = created.id
        } else {
          const created = await invoke<{ id: string }>('create_auth_config', {
            ac: {
              id: '',
              name: authName,
              auth_type: authType,
              auth_data: JSON.stringify(authData),
              created_at: '',
              updated_at: '',
            },
          })
          savedAuthConfigId = created.id
        }
      } catch (authErr) {
        console.warn('[NetworkTab] 创建认证配置失败，继续保存网络配置:', authErr)
      }
    }

    // 创建网络配置（根据作用域选择 API）
    if (isProject && projectPath) {
      const createdNc = await invoke<NetworkProfile>('project_create_network_config', {
        name: f.name || `未命名-${hop.protocol}`,
        networkType: hop.protocol,
        config: buildConfigJson(hop.protocol, f),
        authConfigId: savedAuthConfigId,
        projectPath,
      })
      await loadAllProject(projectPath)
      hop.profileId = createdNc.id
    } else {
      const cfg: Record<string, unknown> = {
        name: f.name || `未命名-${hop.protocol}`,
        network_type: hop.protocol,
        origin: 'global',
        config: buildConfigJson(hop.protocol, f),
      }
      if (savedAuthConfigId) (cfg as Record<string, unknown>).auth_config_id = savedAuthConfigId
      const createdNc = await invoke<NetworkProfile>('create_network_config', { nc: cfg })
      await loadAll()
      hop.profileId = createdNc.id
    }

    hop.mode = 'select'
  } catch (e) {
    console.error('[NetworkTab] Failed to create profile:', e)
  } finally {
    creating[id] = false
  }
}

function buildConfigJson(protocol: string, f: NewFormFields): string {
  let cfg: Record<string, unknown>
  if (protocol === 'ssh') {
    cfg = {
      host: f.host || 'localhost',
      port: f.port || 22,
      username: f.username || 'root',
      authType: f.authType || 'password',
      password: f.authType === 'password' ? f.password || '' : undefined,
      keyPath: f.authType === 'key' ? f.keyPath || '' : undefined,
      localPort: f.localPort,
      remoteHost: f.remoteHost || undefined,
      remotePort: f.remotePort,
      keepAlive: f.keepAlive ?? 60,
    }
  } else if (protocol === 'ssl') {
    cfg = {
      mode: f.sslMode || 'verify-full',
      ca: f.ca || undefined,
      cert: f.cert || undefined,
      key: f.key || undefined,
    }
  } else {
    cfg = {
      type: f.proxyType || 'socks5',
      host: f.host || 'proxy.corp.com',
      port: f.port || 1080,
      username: f.username || undefined,
      password: f.password || undefined,
    }
  }
  return JSON.stringify(cfg)
}

// ===== Drag helpers (composable + DOM style) =====
function dragStart(e: DragEvent, id: string) {
  composableDragStart(id)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}
function dragOver(e: DragEvent) {
  if (e.dataTransfer) {
    e.dataTransfer.dropEffect = 'move'
    ;(e.currentTarget as HTMLElement).classList.add('drag-over')
  }
}
function dragLeave(e: DragEvent) {
  ;(e.currentTarget as HTMLElement).classList.remove('drag-over')
}
function dragEnd() {
  composableDragEnd()
  document.querySelectorAll('.chain-item').forEach(el => el.classList.remove('drag-over'))
}
function drop(_e: DragEvent, targetId: string) {
  document.querySelectorAll('.chain-item').forEach(el => el.classList.remove('drag-over'))
  composableDrop(targetId)
}

// ==================== Profile Manager Modal ====================

const showProfileMgr = ref(false)
const profileMgrTab = ref<'ssh' | 'ssl' | 'proxy'>('ssh')

function openProfileMgr(hop: Hop) {
  activeProfileMgrHop.value = hop
  profileMgrTab.value = hop.protocol
  showProfileMgr.value = true
}

// ==================== Lifecycle & Watch ====================

onMounted(async () => {
  await loadAll()
  if (props.scope?.project) {
    const pp = await getProjectPath()
    if (pp) await loadAllProject(pp)
  }
  loadSavedAuthConfigs()
  // Ensure forms exist for initial hops
  chain.value.forEach(h => ensureForm(h.id))
})

watch(
  () => props.scope?.project,
  async isProject => {
    if (isProject) {
      const pp = await getProjectPath()
      if (pp) await loadAllProject(pp)
    } else {
      // 项目作用域取消时，重新加载全局配置，清除项目级配置缓存
      await loadAll()
    }
  }
)

watch(
  chain,
  () => {
    const enabledProfileIds = chain.value
      .filter(h => h.enabled)
      .map(h => {
        if (h.mode === 'select' && h.profileId) return h.profileId
        if (h.mode === 'new' && h.id) return `new:${h.id}`
        if (h.mode === 'custom' && h.id) return `custom:${h.id}`
        return null
      })
      .filter((x): x is string => x !== null)

    const customConfigs: Record<string, unknown> = {}
    chain.value
      .filter(h => h.mode === 'custom' && h.enabled)
      .forEach(h => {
        if (h.id && customData[h.id]) {
          const vals = customData[h.id]
          customConfigs[h.id] = JSON.parse(buildConfigJson(h.protocol, vals))
        }
      })

    emit('extra-config', {
      networkConfigId: enabledProfileIds.length > 0 ? enabledProfileIds.join(',') : null,
      customConfigs: Object.keys(customConfigs).length > 0 ? customConfigs : undefined,
    })
  },
  { deep: true }
)

// ==================== Chain Hop Test Connection ====================

const testingHop = reactive<Record<string, boolean>>({})

async function testChainHop(hop: Hop) {
  testingHop[hop.id] = true
  try {
    if (hop.mode === 'select' && hop.profileId) {
      // Test saved network config on backend
      const params: Record<string, unknown> = { networkConfigId: hop.profileId }
      // 传递项目路径，支持项目级网络配置测试
      const pp = await getProjectPath()
      if (pp) params.projectPath = pp
      const result = await invoke<{ success: boolean; message: string; response_time_ms: number }>(
        'test_network_config',
        params
      )
      if (result.success) {
        message.success(
          `🧪 ${hop.protocol.toUpperCase()} ${t('navigator.testSuccess')}\n${result.message}\n延迟: ${result.response_time_ms}ms`
        )
      } else {
        message.warning(
          `❌ ${hop.protocol.toUpperCase()} ${t('navigator.testFailed')}\n${result.message}`
        )
      }
    } else if (hop.mode === 'new') {
      // Save the new form first, then test
      await saveNewProfile(hop)
      if (hop.profileId) {
        await testChainHop(hop)
        return
      }
      message.warning(`⚠️ ${t('navigator.pleaseSaveFirst') || '请先保存配置'}`)
    } else {
      message.warning(
        `⚠️ ${t('navigator.customCannotTest') || '一次性自定义配置不支持单独测试，请保存后再测试'}`
      )
    }
  } catch (e) {
    message.error(`❌ ${t('navigator.testFailed')}: ${e instanceof Error ? e.message : String(e)}`)
  } finally {
    testingHop[hop.id] = false
  }
}

// SSH auth type options (real names)
const sshAuthTypeOpts = [
  { label: '🔑 SSH 密码认证', value: 'ssh_password' },
  { label: '🔐 公钥认证 (RSA/ED25519/ECDSA)', value: 'ssh_private_key' },
]

const savedAuthConfigs = ref<AuthConfig[]>([])

/** Dynamic auth config select options — fetched from backend */
const chainAuthCfgOpts = computed(() => {
  const opts: { label: string; value: string }[] = [{ label: '— 手动填写 —', value: '' }]
  for (const a of savedAuthConfigs.value) {
    const scopeIcon = a.scope === 'global' ? '🌐' : '📝'
    opts.push({ label: `${a.name} · ${scopeIcon}`, value: a.id })
  }
  return opts
})

/** Load saved auth configs — merges global + project when scope includes project */
async function loadSavedAuthConfigs() {
  try {
    const globalConfigs = await invoke<AuthConfig[]>('list_auth_configs')
    savedAuthConfigs.value = globalConfigs

    if (props.scope?.project) {
      const projectPath = await getProjectPath()
      if (projectPath) {
        const projectConfigs = await invoke<AuthConfig[]>('project_list_auth_configs', {
          projectPath,
        })
        savedAuthConfigs.value = [...globalConfigs, ...projectConfigs]
      }
    }
  } catch {
    console.warn('[NetworkTab] list_auth_configs unavailable, auth config picker disabled')
  }
}

function onChainAuthChange(hopId: string, _val: string) {
  // Reset saved auth when auth type changes
  if (newFormData[hopId]) newFormData[hopId].savedAuthId = ''
}
</script>

<style scoped>
.net-tab {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 2px 0;
}
.net-file-hint {
  text-align: center;
  padding: 40px 20px;
}
.net-file-hint .hint-icon {
  opacity: 0.3;
  margin-bottom: 12px;
}
.net-file-hint h3 {
  font-size: 15px;
  color: var(--color-text-secondary);
  margin-bottom: 6px;
}
.net-file-hint p {
  font-size: 12px;
  color: var(--color-text-muted);
  line-height: 1.6;
}
.empty-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 120px;
  font-size: 13px;
  color: var(--color-text-muted);
}

.net-hint {
  font-size: 12px;
  color: var(--color-text-muted);
  padding: 8px 12px;
  background: var(--color-bg-elevated);
  border: 1px dashed var(--color-border-subtle);
  border-radius: 6px;
  line-height: 1.6;
  text-align: center;
}
.net-hint strong {
  color: var(--brand-accent);
}

.chain-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 10px;
  font-weight: 600;
  color: var(--color-text-muted);
  text-transform: uppercase;
  padding: 0 2px;
}
.ch-drag {
  width: 26px;
}
.ch-order {
  width: 30px;
  text-align: center;
}
.ch-type {
  width: 90px;
}
.ch-config {
  flex: 1;
}
.ch-toggle {
  width: 48px;
  text-align: center;
}
.ch-acts {
  width: 56px;
  text-align: center;
}

.chain-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.chain-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 6px 4px;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-subtle);
  border-radius: 6px;
  transition: all 0.15s;
}
.chain-item:hover {
  border-color: var(--color-border);
}
.chain-item.disabled {
  opacity: 0.45;
}
.chain-item.drag-over {
  border-color: var(--brand-accent);
  background: var(--brand-accent-soft);
}
.chain-item.ssl {
  position: relative;
  border-left: 3px solid var(--brand-accent);
}
.chain-item.ssl::after {
  content: '末尾层';
  position: absolute;
  top: -6px;
  right: 8px;
  font-size: 9px;
  padding: 1px 5px;
  border-radius: 3px;
  background: rgba(137, 180, 250, 0.12);
  color: var(--brand-accent);
}

.drag-handle {
  width: 26px;
  text-align: center;
  cursor: grab;
  color: var(--color-text-muted);
  font-size: 14px;
  user-select: none;
  padding-top: 2px;
}
.drag-handle:active {
  cursor: grabbing;
}

.order-badge {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  font-weight: 700;
  flex-shrink: 0;
}
.order-badge.ssh {
  background: rgba(166, 227, 161, 0.1);
  border: 2px solid rgba(166, 227, 161, 0.2);
  color: var(--brand-success);
}
.order-badge.ssl {
  background: rgba(137, 180, 250, 0.1);
  border: 2px solid rgba(137, 180, 250, 0.2);
  color: var(--brand-accent);
}
.order-badge.proxy {
  background: rgba(250, 179, 135, 0.1);
  border: 2px solid rgba(250, 179, 135, 0.2);
  color: var(--brand-warning);
}

.type-badge {
  width: 90px;
  padding: 3px 8px;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}
.type-badge.ssh {
  background: rgba(166, 227, 161, 0.08);
  color: var(--brand-success);
}
.type-badge.ssl {
  background: rgba(137, 180, 250, 0.08);
  color: var(--brand-accent);
}
.type-badge.proxy {
  background: rgba(250, 179, 135, 0.08);
  color: var(--brand-warning);
}

.config-area {
  flex: 1;
  min-width: 0;
}
.config-select {
  max-width: 240px;
}

/* Select row */
.select-row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.select-row .config-select {
  flex: 1;
  max-width: none;
}
.custom-link {
  font-size: 10px;
  flex-shrink: 0;
}

/* File input with browse button */
.file-input-row {
  display: flex;
  gap: 4px;
}
.file-input-row .n-input {
  flex: 1;
}

.new-save-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 2px;
}
.forward-info {
  margin-top: 4px;
}
.forward-tag {
  font-size: 10px;
  color: var(--color-text-muted);
}

.toggle-wrap {
  width: 48px;
  display: flex;
  justify-content: center;
  padding-top: 4px;
}
.act-wrap {
  width: 56px;
  display: flex;
  gap: 2px;
  justify-content: center;
  align-items: flex-start;
  padding-top: 2px;
}
.del-btn:hover :deep(svg) {
  color: var(--brand-danger);
}
.no-del {
  opacity: 0.25;
  cursor: not-allowed;
  font-size: 12px;
}

.net-warning {
  font-size: 11px;
  padding: 8px 12px;
  background: rgba(249, 226, 175, 0.06);
  border: 1px solid rgba(249, 226, 175, 0.15);
  border-radius: 6px;
  color: var(--brand-warning);
}

.add-hop-row {
  position: relative;
  display: flex;
  gap: 8px;
  align-items: center;
}
.hop-limit {
  font-size: 11px;
  color: var(--color-text-muted);
}

.hop-menu {
  position: absolute;
  top: 100%;
  left: 0;
  z-index: 10;
  min-width: 220px;
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border-subtle);
  border-radius: 6px;
  box-shadow: 0 8px 24px var(--color-bg-primary);
  overflow: hidden;
}
.hop-opt {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  cursor: pointer;
  font-size: 12px;
  color: var(--color-text-secondary);
  transition: background 0.1s;
}
.hop-opt:hover {
  background: var(--color-hover);
}
.hop-menu-sep {
  height: 1px;
  background: var(--color-border-subtle);
  margin: 4px 0;
}
.hop-menu-desc {
  padding: 6px 12px 8px;
  font-size: 10px;
  color: var(--color-text-muted);
  line-height: 1.5;
}

.net-empty {
  text-align: center;
  padding: 16px;
  color: var(--color-text-muted);
  font-size: 12px;
}

/* ===== Inline forms (aligned with prototype v5) ===== */
.inline-form-v5 {
  padding: 12px;
  margin-top: 4px;
  background: var(--color-bg-surface);
  border: 1px solid var(--brand-accent);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.inline-form-v5.custom {
  border-color: var(--brand-warning);
}
.custom-hint {
  font-size: 10px;
  color: var(--brand-warning);
  margin-bottom: 2px;
}

.form-row {
  display: flex;
  gap: 8px;
  align-items: flex-end;
}
.form-group {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.form-group.f1 {
  flex: 1;
}
.form-group.f2 {
  flex: 2;
}
.form-label {
  font-size: 11px;
  font-weight: 500;
  color: var(--color-text-secondary);
}
.form-section-label {
  font-size: 10px;
  font-weight: 600;
  color: var(--brand-accent);
  letter-spacing: 0.5px;
  padding: 4px 0 2px;
  border-top: 1px solid var(--color-border-subtle);
  margin-top: 2px;
}
.form-hint {
  font-size: 10px;
  color: var(--color-text-muted);
  line-height: 1.4;
}
.profile-scope-badge {
  font-size: 11px;
  color: var(--color-text-secondary);
  padding: 2px 8px;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-subtle);
  border-radius: 4px;
  align-self: flex-start;
  margin-top: 2px;
}

/* Test connection button in chain save row */
.btn-test-conn {
  white-space: nowrap;
}
</style>
