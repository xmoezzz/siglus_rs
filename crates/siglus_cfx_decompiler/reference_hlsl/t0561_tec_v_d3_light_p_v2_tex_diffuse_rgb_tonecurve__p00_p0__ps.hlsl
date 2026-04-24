uniform sampler2D s0;
uniform sampler2D s1;
uniform sampler2D s2;

static const float4 c12 = float4(0.0005, 1.0, 0.5, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
    float4 t1 : TEXCOORD1;
    float4 t2 : TEXCOORD2;
    float4 t3 : TEXCOORD3;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r4 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = (1.0 / input.t3.w);
    r0.x = (input.t3.x * r0.w + c12.y);
    r0.y = (input.t3.y * r0.w + c12.y);
    r0.x = (r0.x * c6.z);
    r0.z = c12.z;
    r0.z = (r0.x * r0.z + c6.x);
    r0.x = (r0.y * -(c12.z) + c12.y);
    r0.xy = (r0.zx * c6.ww + c6.yy);
    r0 = tex2D(s2, r0.xy);
    r1 = tex2D(s0, input.t0.xy);
    r2.xyz = (-(input.t2.xyz) + c3.xyz);
    r2.x = dot(r2.xyz, r2.xyz);
    r2.x = rsqrt(r2.x);
    r2.x = (1.0 / r2.x);
    r2.x = (r2.x + -(c7.y));
    r2.x = saturate((r2.x * c1.x));
    r3.xyz = (-(input.t2.xyz) + c4.xyz);
    r3.w = dot(r3.xyz, r3.xyz);
    r3.w = rsqrt(r3.w);
    r3.xyz = (r3.xyz * r3.www);
    r3.w = (1.0 / r3.w);
    r4.xyz = normalize(input.t1.xyz);
    r2.y = dot(r4.xyz, r3.xyz);
    r2.z = (r3.w * -(c12.x) + c12.y);
    r2.y = saturate((r2.y * r2.z));
    r1 = (r1 * input.v0);
    r3 = (r2.yyyy * r1);
    r3 = (r3 * c5);
    r3 = (c0.xxxx >= 0 ? r3 : r1);
    r4 = lerp(r3, r0, r2.xxxx);
    r0 = (c2.xxxx >= 0 ? r4 : r3);
    r0.w = dot(c8, r0);
    r2.xyz = lerp(r0.xyz, r0.www, c11.yyy);
    r2.w = c11.x;
    r0.x = r2.x;
    r0.y = r2.w;
    r2.x = r2.y;
    r2.y = r2.w;
    r3.x = r2.z;
    r3.y = r2.w;
    r0 = tex2D(s1, r0.xy);
    r2 = tex2D(s1, r2.xy);
    r3 = tex2D(s1, r3.xy);
    r3.x = r0.x;
    r3.y = r2.y;
    r0.xyz = lerp(r3.xyz, c9.xyz, c9.www);
    r1.xyz = (r0.xyz + c10.xyz);
    output.oC0 = r1;
    return output;
}
