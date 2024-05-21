<script setup>
import { onMounted, computed } from "vue";
import { DateTime } from "luxon";

const props = defineProps({
  path: {
    type: String,
    required: true,
  }
})

import { ref } from 'vue'

const detail = ref({
  document: ["……"]
})

onMounted(() => {
  console.log(`Mounted PostDetail for path ${props.path}`)
  fetch(`posts/${props.path}/detail.json`)
    .then((resp) => resp.json())
    .then((value) => {
      detail.value = value
    })
})

const publishedAt = computed(() => DateTime.fromISO(detail.value.revisions.at(0)))
const lastModifiedAt = computed(() => DateTime.fromISO(detail.value.revisions.at(-1)))
</script>

<template>
  <h1 v-if="Object.hasOwn(detail, 'title')">{{ detail.title }}</h1>
  <div v-if="Object.hasOwn(detail, 'revisions')">
    {{ publishedAt.toISO() }}
    <span v-if="detail.revisions.length > 1">
      （最近修订 {{ lastModifiedAt.toISO() }}）
    </span>
  </div>
  <hr>
  <div v-for="item in detail.document" :key="item">
    {{ item }}
  </div>
</template>