<script setup>
import { onMounted } from "vue";

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
  detail.value = {
    title: "这是雪豹",
    document: ["雪豹闭嘴。"]
  }
})
</script>

<template>
  <h1 v-if="Object.hasOwn(detail, 'title')">{{ detail.title }}</h1>
  <div v-for="item in detail.document" :key="item">
    {{ item }}
  </div>
</template>